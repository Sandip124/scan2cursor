// Decodes camera frames off the main thread, so the page never stutters while scanning.
// Uses the phone's built-in barcode reader when the browser has one (Chrome on Android), otherwise
// zxing-cpp compiled to WebAssembly (iPhone, Firefox), served by this computer, never from the internet.
importScripts('/vendor/barcode-detector.js');

const FORMATS = ['ean_13', 'ean_8', 'upc_a', 'upc_e', 'code_128', 'code_39', 'code_93', 'itf', 'qr_code', 'data_matrix'];

BarcodeDetectionAPI.prepareZXingModule({
  overrides: { locateFile: (path, prefix) => (path.endsWith('.wasm') ? '/vendor/zxing_reader.wasm' : prefix + path) },
});

let detector = null;
let engine = '';
const ready = (async () => {
  // ?engine=zxing (passed on from the scanner link) skips the built-in reader: for troubleshooting.
  const forceZxing = new URLSearchParams(self.location.search).get('engine') === 'zxing';
  if (!forceZxing && 'BarcodeDetector' in self) {
    try {
      const supported = await self.BarcodeDetector.getSupportedFormats();
      const formats = FORMATS.filter(f => supported.includes(f));
      // Only use the built-in reader if it covers the common retail and shipping codes.
      if (formats.includes('ean_13') && formats.includes('code_128') && formats.includes('qr_code')) {
        detector = new self.BarcodeDetector({ formats });
        engine = 'built-in';
        return;
      }
    } catch {}
  }
  detector = new BarcodeDetectionAPI.BarcodeDetector({ formats: FORMATS });
  engine = 'zxing';
})();

self.onmessage = async e => {
  const { frame } = e.data;
  let codes = [];
  let error = null;
  try {
    await ready;
    const found = await detector.detect(frame);
    codes = found.map(b => ({ code: b.rawValue, format: b.format }));
  } catch (err) {
    error = String(err && err.message ? err.message : err);
  } finally {
    if (frame && frame.close) frame.close(); // free the frame's memory right away
  }
  self.postMessage({ codes, engine, error });
};
