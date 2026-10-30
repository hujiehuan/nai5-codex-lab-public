-- v0.1.4：entry_image.sha256（CONTRACT §8.1 images[].sha256）。展示图落地后由 sync/images.rs 计算写入；
-- 002 之前下载的行为 NULL，完整池导出时由 export::backfill_image_sha256 补算写回。
ALTER TABLE entry_image ADD COLUMN sha256 TEXT;
