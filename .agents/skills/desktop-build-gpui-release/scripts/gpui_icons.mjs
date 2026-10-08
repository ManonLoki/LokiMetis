/** 使用 Node 标准库把已确认的 PNG 生成项目本地 ICNS/ICO，无全局图像依赖。 */
import { deflateSync, inflateSync } from 'node:zlib';

const signature = Buffer.from([137,80,78,71,13,10,26,10]);
/** PNG chunk 的标准 CRC-32。 */
function crc32(bytes) {
  let crc = 0xffffffff;
  for (const byte of bytes) {
    crc ^= byte;
    for (let bit = 0; bit < 8; bit += 1) crc = (crc >>> 1) ^ ((crc & 1) ? 0xedb88320 : 0);
  }
  return (crc ^ 0xffffffff) >>> 0;
}
/** 序列化带长度与摘要的 PNG chunk。 */
function chunk(type, bytes) {
  const header = Buffer.alloc(8); header.writeUInt32BE(bytes.length); header.write(type, 4, 'ascii');
  const tail = Buffer.alloc(4); tail.writeUInt32BE(crc32(Buffer.concat([header.subarray(4), bytes])));
  return Buffer.concat([header, bytes, tail]);
}

/** 只接受有界、非隔行、8-bit RGB/RGBA，其他合法 PNG 明确要求预先标准化。 */
export function decodePng(source) {
  if (!Buffer.isBuffer(source) || source.length > 8 * 1024 * 1024 || !source.subarray(0,8).equals(signature)) throw new Error('icon source must be a PNG <= 8 MiB');
  let cursor = 8; let header; const compressed = []; let ended = false;
  while (cursor < source.length) {
    if (cursor + 12 > source.length) throw new Error('truncated PNG');
    const length = source.readUInt32BE(cursor); const type = source.toString('ascii',cursor + 4,cursor + 8);
    if (cursor + length + 12 > source.length) throw new Error('truncated PNG chunk');
    const bytes = source.subarray(cursor + 8,cursor + 8 + length);
    if (crc32(source.subarray(cursor + 4,cursor + 8 + length)) !== source.readUInt32BE(cursor + 8 + length)) throw new Error('PNG checksum failed');
    if (type === 'IHDR') { if (header || cursor !== 8 || length !== 13) throw new Error('invalid PNG IHDR'); header = bytes; }
    else if (type === 'IDAT') compressed.push(bytes);
    else if (type === 'IEND') { if (length || cursor + 12 !== source.length) throw new Error('invalid PNG IEND'); ended = true; }
    else if (/^[A-Z]/u.test(type) && type !== 'PLTE') throw new Error('unsupported critical PNG chunk');
    cursor += length + 12;
  }
  if (!header || !ended || !compressed.length) throw new Error('incomplete PNG');
  const width = header.readUInt32BE(0), height = header.readUInt32BE(4), color = header[9], channels = color === 6 ? 4 : 3;
  if (width !== height || width < 256 || width > 2048 || header[8] !== 8 || ![2,6].includes(color) || header[10] || header[11] || header[12]) throw new Error('icon source requires square 256..2048px noninterlaced RGB/RGBA PNG');
  const stride = width * channels;
  const filtered = inflateSync(Buffer.concat(compressed), { maxOutputLength: (stride + 1) * height });
  if (filtered.length !== (stride + 1) * height) throw new Error('invalid PNG scanline size');
  const raw = Buffer.alloc(stride * height);
  for (let y = 0; y < height; y += 1) {
    const filter = filtered[y * (stride + 1)]; if (filter > 4) throw new Error('invalid PNG filter');
    for (let x = 0; x < stride; x += 1) {
      const left = x >= channels ? raw[y * stride + x - channels] : 0;
      const up = y ? raw[(y - 1) * stride + x] : 0;
      const diagonal = y && x >= channels ? raw[(y - 1) * stride + x - channels] : 0;
      const prediction = left + up - diagonal;
      const distances = [Math.abs(prediction - left),Math.abs(prediction - up),Math.abs(prediction - diagonal)];
      const paeth = distances[0] <= distances[1] && distances[0] <= distances[2] ? left : distances[1] <= distances[2] ? up : diagonal;
      const predictor = [0,left,up,Math.floor((left + up) / 2),paeth][filter];
      raw[y * stride + x] = (filtered[y * (stride + 1) + 1 + x] + predictor) & 255;
    }
  }
  return { width, height, channels, pixels: raw };
}

/** 编码无滤波 RGBA PNG；供跨平台图标容器使用。 */
export function encodePng(width, height, pixels) {
  const header = Buffer.alloc(13); header.writeUInt32BE(width); header.writeUInt32BE(height,4); header[8] = 8; header[9] = 6;
  const scanlines = Buffer.alloc((width * 4 + 1) * height);
  for (let y = 0; y < height; y += 1) pixels.copy(scanlines,y * (width * 4 + 1) + 1,y * width * 4,(y + 1) * width * 4);
  return Buffer.concat([signature,chunk('IHDR',header),chunk('IDAT',deflateSync(scanlines)),chunk('IEND',Buffer.alloc(0))]);
}

/** 图标使用固定中心采样，保持同一 Logo 的颜色和透明度。 */
function resized(image, size) {
  const pixels = Buffer.alloc(size * size * 4);
  for (let y = 0; y < size; y += 1) for (let x = 0; x < size; x += 1) {
    const sx = Math.min(image.width - 1,Math.floor((x + 0.5) * image.width / size));
    const sy = Math.min(image.height - 1,Math.floor((y + 0.5) * image.height / size));
    for (let channel = 0; channel < 4; channel += 1) pixels[(y * size + x) * 4 + channel] = channel === 3 && image.channels === 3 ? 255 : image.pixels[(sy * image.width + sx) * image.channels + channel];
  }
  return encodePng(size,size,pixels);
}

/** 同一已选 Logo 的多尺寸平台容器，不改变品牌身份。 */
export function renderPlatformIcons(source) {
  const image = decodePng(source);
  const sizes = [32,48,64,128,256]; const images = sizes.map(size => resized(image,size));
  const icoHeader = Buffer.alloc(6); icoHeader.writeUInt16LE(1,2); icoHeader.writeUInt16LE(sizes.length,4);
  let offset = 6 + sizes.length * 16;
  const entries = sizes.map((size,index) => {
    const entry = Buffer.alloc(16); entry[0] = size === 256 ? 0 : size; entry[1] = entry[0]; entry.writeUInt16LE(1,4); entry.writeUInt16LE(32,6); entry.writeUInt32LE(images[index].length,8); entry.writeUInt32LE(offset,12); offset += images[index].length; return entry;
  });
  const icnsEntries = [[128,'ic07'],[256,'ic08'],[512,'ic09'],[1024,'ic10']].map(([size,type]) => {
    const png = resized(image,size); const header = Buffer.alloc(8); header.write(type,0,'ascii'); header.writeUInt32BE(png.length + 8,4); return Buffer.concat([header,png]);
  });
  const icnsHeader = Buffer.alloc(8); icnsHeader.write('icns'); icnsHeader.writeUInt32BE(8 + icnsEntries.reduce((sum,item) => sum + item.length,0),4);
  return { macos: Buffer.concat([icnsHeader,...icnsEntries]), windows: Buffer.concat([icoHeader,...entries,...images]) };
}

/** 中性拖拽背景不携带示例产品名。 */
export function renderDmgBackground() {
  const width = 660, height = 440, pixels = Buffer.alloc(width * height * 4);
  for (let y = 0; y < height; y += 1) for (let x = 0; x < width; x += 1) {
    const index = (y * width + x) * 4;
    const arrow = (x >= 300 && x <= 350 && Math.abs(y - 220) <= 3) || (x >= 337 && x <= 353 && Math.abs(y - 220) <= 353 - x);
    const shade = arrow ? 135 : Math.round(250 - y / height * 10);
    pixels[index] = shade; pixels[index + 1] = shade; pixels[index + 2] = arrow ? shade : Math.min(255,shade + 2); pixels[index + 3] = 255;
  }
  return encodePng(width,height,pixels);
}
