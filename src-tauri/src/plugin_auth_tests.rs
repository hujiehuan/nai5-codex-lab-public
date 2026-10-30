//! Loopback HTTP verifies the actual transport; no real account or bot involved.
use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Server { base:String, task:tokio::task::JoinHandle<()> }
impl Drop for Server {fn drop(&mut self){self.task.abort();}}
async fn server(reply:impl Fn(&str,&str,&str)->(u16,Value)+Send+Sync+'static)->Server {
    let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base=format!("http://{}",listener.local_addr().unwrap());let reply=Arc::new(reply);
    let task=tokio::spawn(async move {let mut connections=tokio::task::JoinSet::new();loop{
        tokio::select! {accepted=listener.accept()=>{let (mut socket,_)=accepted.unwrap();let reply=reply.clone();connections.spawn(async move {
            let mut bytes=Vec::new();let mut buf=[0u8;4096];let (header,end)=loop{
                let n=socket.read(&mut buf).await.unwrap();if n==0{return;}bytes.extend_from_slice(&buf[..n]);
                if let Some(end)=bytes.windows(4).position(|w|w==b"\r\n\r\n") {break (String::from_utf8_lossy(&bytes[..end]).to_string(),end+4);}
            };
            let len=header.lines().find_map(|line|line.to_lowercase().strip_prefix("content-length:").and_then(|v|v.trim().parse::<usize>().ok())).unwrap_or(0);
            while bytes.len()<end+len {let n=socket.read(&mut buf).await.unwrap();if n==0{return;}bytes.extend_from_slice(&buf[..n]);}
            let path=header.lines().next().unwrap().split_whitespace().nth(1).unwrap();let body=String::from_utf8_lossy(&bytes[end..end+len]);
            let (status,data)=reply(path,&header,&body);let data=data.to_string();
            let response=format!("HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}",data.len());
            let _=socket.write_all(response.as_bytes()).await;
        });},_ = connections.join_next(), if !connections.is_empty()=>{}}
    }});Server{base,task}
}
fn conn(s:&Server)->PluginConn{PluginConn{base:s.base.clone(),username:"fixture".into(),password:"not-a-real-password".into(),library_token:None}}
async fn get(http:&reqwest::Client,c:&PluginConn)->AppResult<(reqwest::StatusCode,String)>{plugin_request(http,c,reqwest::Method::GET,"/status",&[],None,Duration::from_secs(5)).await}

#[tokio::test]
async fn twelve_parallel_status_queries_use_one_login_and_keep_authentication() {
    let logins=Arc::new(AtomicUsize::new(0));let count=logins.clone();
    let s=server(move |path,header,body|if path.ends_with("login"){
        assert_eq!(serde_json::from_str::<Value>(body).unwrap()["password"],"not-a-real-password");count.fetch_add(1,Ordering::SeqCst);(200,json!({"data":{"token":"first"}}))
    }else{assert!(header.to_lowercase().contains("authorization: bearer first"));(200,json!({"ok":true}))}).await;
    let http=reqwest::Client::new();let c=conn(&s);
    let results=futures_util::future::join_all((0..12).map(|_|get(&http,&c))).await;
    assert!(results.iter().all(|r|r.as_ref().unwrap().0.is_success()));assert_eq!(logins.load(Ordering::SeqCst),1);
    let session=PluginSession::new(c.clone());session.request(&http,reqwest::Method::GET,"/status",&[],None,Duration::from_secs(5)).await.unwrap();
    assert_eq!(logins.load(Ordering::SeqCst),1,"batch and settings share the credential identity");
}

#[tokio::test]
async fn unauthorized_refreshes_once_and_changed_password_uses_a_new_identity() {
    let logins=Arc::new(AtomicUsize::new(0));let count=logins.clone();
    let s=server(move|path,header,_|if path.ends_with("login"){
        let n=count.fetch_add(1,Ordering::SeqCst)+1;(200,json!({"data":{"token":format!("token-{n}")}}))
    }else if header.to_lowercase().contains("bearer token-1"){(401,json!({"error":"expired"}))}else{(200,json!({}))}).await;
    let http=reqwest::Client::new();let mut c=conn(&s);
    assert!(get(&http,&c).await.unwrap().0.is_success());assert_eq!(logins.load(Ordering::SeqCst),2);
    assert!(get(&http,&c).await.unwrap().0.is_success());assert_eq!(logins.load(Ordering::SeqCst),2);
    c.password="changed-fixture-password".into();get(&http,&c).await.unwrap();assert_eq!(logins.load(Ordering::SeqCst),3);
}

#[tokio::test]
async fn rate_limit_does_not_fall_back_to_md5_or_repeat_login_on_page_switch() {
    let logins=Arc::new(AtomicUsize::new(0));let count=logins.clone();
    let s=server(move|path,_,_|{assert!(path.ends_with("login"));count.fetch_add(1,Ordering::SeqCst);(429,json!({"message":"too frequent"}))}).await;
    let http=reqwest::Client::new();let c=conn(&s);
    for _ in 0..4 {assert!(get(&http,&c).await.unwrap_err().message.contains("429"));}
    assert_eq!(logins.load(Ordering::SeqCst),1);
}

#[tokio::test]
async fn legacy_md5_fallback_occurs_only_after_plaintext_401() {
    let passwords=Arc::new(parking_lot::Mutex::new(Vec::new()));let received=passwords.clone();
    let s=server(move|path,_,body|if path.ends_with("login"){
        let p=serde_json::from_str::<Value>(body).unwrap()["password"].as_str().unwrap().to_owned();received.lock().push(p.clone());
        if p=="not-a-real-password" {(401,json!({}))}else{(200,json!({"data":{"token":"legacy"}}))}
    }else{(200,json!({}))}).await;
    get(&reqwest::Client::new(),&conn(&s)).await.unwrap();
    assert_eq!(*passwords.lock(),vec!["not-a-real-password".to_string(),password_for_login("not-a-real-password")]);
}
