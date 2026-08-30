import { readFile, writeFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, '..');
const pngPath = path.join(root, 'src-tauri', 'icons', '128x128@2x.png');
const icoPath = path.join(root, 'src-tauri', 'icons', 'icon.ico');

// Windows ICO files may contain PNG-compressed images. Tauri only needs a
// valid .ico at build time, so we wrap the generated 256x256 PNG in a
// single-image ICO container instead of requiring another binary in Git.
const png = await readFile(pngPath);
const header = Buffer.alloc(22);

// ICONDIR
header.writeUInt16LE(0, 0); // reserved
header.writeUInt16LE(1, 2); // type = icon
header.writeUInt16LE(1, 4); // image count

// ICONDIRENTRY (256 is encoded as 0 for width/height)
header.writeUInt8(0, 6);
header.writeUInt8(0, 7);
header.writeUInt8(0, 8); // palette
header.writeUInt8(0, 9); // reserved
header.writeUInt16LE(1, 10); // color planes
header.writeUInt16LE(32, 12); // bits per pixel
header.writeUInt32LE(png.length, 14);
header.writeUInt32LE(22, 18); // image offset

await writeFile(icoPath, Buffer.concat([header, png]));
console.log(`[NOVA] Windows icon generated: ${icoPath}`);
