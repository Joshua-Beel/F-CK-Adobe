import { mkdirSync, writeFileSync } from 'node:fs';
import { deflateSync } from 'node:zlib';
import { fileURLToPath } from 'node:url';
const root = fileURLToPath(new URL('../', import.meta.url));
function pdf(count, scan) {
  const objects = [];
  const add = value => { objects.push(Buffer.isBuffer(value) ? value : Buffer.from(value)); return objects.length; };
  const stream = (dict, data) => Buffer.concat([Buffer.from(`<< ${dict} /Length ${data.length} >>\nstream\n`), data, Buffer.from('\nendstream')]);
  add(''); add('');
  const font = add('<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>');
  let image;
  if (scan) {
    const width = 1224, height = 1584, pixels = Buffer.alloc(width * height, 250);
    for (let y = 130; y < 1460; y++) for (let x = 100; x < 1120; x++) {
      if ((y % 46 < 3) || (x % 245 < 2) || (y % 46 > 15 && y % 46 < 23 && x % 16 < 10 && x % 245 > 12)) pixels[y * width + x] = 35;
    }
    image = add(stream(`/Type /XObject /Subtype /Image /Width ${width} /Height ${height} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode`, deflateSync(pixels)));
  }
  const kids = [];
  const lines = [
    ['A place for your PDFs.', 'Open. Read. Navigate.', 'This sample document is included with the viewer foundation.', 'Use the All tools panel on the left to explore the planned tools.', 'Page controls and navigation live on the right.', 'Your original files are opened read-only.'],
    ['Move through a document.', 'Your pages, within reach.', 'Use the page number field to jump to a specific page.', 'Page Up and Page Down move between pages.', 'Home and End take you to the first and last page.', 'Open the Pages panel on the right for a page list.'],
    ['Make room to read.', 'A comfortable view.', 'Fit width adapts the page to the space available.', 'Use the zoom controls for a closer look.', 'The hand tool lets you drag the page around.', 'Collapse All tools to give your document more space.'],
    ['Keep your work together.', 'One workspace. Multiple documents.', 'Open another PDF to create a new document tab.', 'Switch between open documents with Ctrl+Tab.', 'Return Home to see files opened in this session.', 'Star a document to find it in the Starred list.'],
    ['Built around local files.', 'The foundation comes first.', 'PDF parsing and rendering happen in the native worker.', 'Only visible pages are rasterized for the viewport.', 'Advanced editing tools are not available in this build.', 'No document upload or account is required.'],
    ['Ready for a real document.', 'Start with a PDF of your own.', 'Choose Open a file or press Ctrl+O.', 'This build supports viewing unencrypted PDFs.', 'Search, OCR, annotations and editing are future milestones.', 'End of the sample document.']
  ];
  for (let page = 0; page < count; page++) {
    let content;
    if (scan) content = `q 612 0 0 792 0 0 cm /Scan Do Q\n`;
    else {
      const text = lines[page % lines.length];
      content = `0.18 0.29 0.40 rg 54 700 35 4 re f\nBT /F1 10 Tf 54 737 Td (PDF WORKSTATION / VIEWER GUIDE) Tj ET\n`;
      content += `BT /F1 29 Tf 54 644 Td (${text[0]}) Tj ET\nBT /F1 17 Tf 54 601 Td (${text[1]}) Tj ET\n`;
      text.slice(2).forEach((line, i) => { content += `0.26 0.28 0.31 rg BT /F1 12 Tf 54 ${535 - i * 35} Td (${line}) Tj ET\n`; });
      content += `0.88 0.9 0.93 rg 54 138 504 110 re f\n0.18 0.29 0.4 rg BT /F1 12 Tf 74 211 Td (LOCAL DOCUMENTS. A FAMILIAR WORKSPACE.) Tj ET\n`;
      content += `BT /F1 10 Tf 74 185 Td (This is a generated sample, not a real customer document.) Tj ET\n`;
    }
    content += `0.35 g BT /F1 10 Tf 54 38 Td (Sample document - Page ${page + 1} of ${count}) Tj ET`;
    const contents = add(stream('', Buffer.from(content)));
    const id = add(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 ${font} 0 R >> ${scan ? `/XObject << /Scan ${image} 0 R >>` : ''} >> /Contents ${contents} 0 R >>`);
    kids.push(`${id} 0 R`);
  }
  objects[0] = Buffer.from('<< /Type /Catalog /Pages 2 0 R >>');
  objects[1] = Buffer.from(`<< /Type /Pages /Kids [${kids.join(' ')}] /Count ${count} >>`);
  const parts = [Buffer.from('%PDF-1.7\n')]; const offsets = [0]; let length = parts[0].length;
  objects.forEach((body, i) => { offsets.push(length); const item = Buffer.concat([Buffer.from(`${i + 1} 0 obj\n`), body, Buffer.from('\nendobj\n')]); parts.push(item); length += item.length; });
  parts.push(Buffer.from(`xref\n0 ${objects.length + 1}\n0000000000 65535 f \n${offsets.slice(1).map(n => `${String(n).padStart(10, '0')} 00000 n \n`).join('')}trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${length}\n%%EOF\n`));
  return Buffer.concat(parts);
}
mkdirSync(root + 'src-tauri/resources', { recursive: true });
mkdirSync(root + 'test-corpus', { recursive: true });
writeFileSync(root + 'src-tauri/resources/welcome.pdf', pdf(6, false));
writeFileSync(root + 'test-corpus/synthetic-scan-98.pdf', pdf(98, true));
writeFileSync(root + 'test-corpus/synthetic-text-1500.pdf', pdf(1500, false));
writeFileSync(root + 'test-corpus/invalid.pdf', 'This is not a PDF.');
console.log('Generated sample and synthetic corpus (6, 98, and 1500 pages).');
