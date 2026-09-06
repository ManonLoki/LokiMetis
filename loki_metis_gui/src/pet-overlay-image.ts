/** 根据魔数推断图片 MIME，供 data URL 使用。 */
export function sniffMonitorImageMime(bytes: number[]): string {
  if (
    bytes.length >= 4 &&
    bytes[0] === 137 &&
    bytes[1] === 80 &&
    bytes[2] === 78 &&
    bytes[3] === 71
  ) {
    return "image/png";
  }
  if (bytes.length >= 3 && bytes[0] === 255 && bytes[1] === 216 && bytes[2] === 255) {
    return "image/jpeg";
  }
  if (bytes.length >= 6 && bytes[0] === 71 && bytes[1] === 73 && bytes[2] === 70) {
    return "image/gif";
  }
  if (
    bytes.length >= 12 &&
    bytes[0] === 82 &&
    bytes[1] === 73 &&
    bytes[2] === 70 &&
    bytes[3] === 70
  ) {
    return "image/webp";
  }
  return "application/octet-stream";
}

/** 把本机图片字节编码为 CSP 已允许的 data URL，避免 blob: 被拦截。 */
export function monitorImageBytesToDataUrl(bytes: number[]): string {
  const mime = sniffMonitorImageMime(bytes);
  const chunkSize = 0x8000;
  let binary = "";
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    const slice = bytes.slice(offset, offset + chunkSize);
    binary += String.fromCharCode(...slice);
  }
  return `data:${mime};base64,${btoa(binary)}`;
}
