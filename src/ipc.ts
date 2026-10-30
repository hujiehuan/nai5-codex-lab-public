import { invoke as tauriInvoke } from "@tauri-apps/api/core";

/** JSON snapshot removes Vue Proxy. Tauri Channel.toJSON preserves its registered callback ID. */
export function snapshot<T>(value: T): T {
  return value === undefined ? value : JSON.parse(JSON.stringify(value));
}

export async function invoke<T>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  const payload = snapshot(args);
  if (import.meta.env.MODE === "mock") {
    const response = await fetch("/__mock__/invoke", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        command,
        args: payload,
        scenario: new URLSearchParams(location.search).get("scenario"),
      }),
    });
    const value = await response.json();
    if (!response.ok) throw value;
    return value as T;
  }
  return tauriInvoke<T>(command, payload);
}
