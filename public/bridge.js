// Drop into any desktop page:
//   <script src="https://localhost:8443/bridge.js"></script>
//   BarcodeBridge.connect({ input: '#sku', token: '<TOKEN>', autoSubmit: true });
(function () {
  const base = (document.currentScript && new URL(document.currentScript.src).host) || location.host;
  window.BarcodeBridge = {
    connect({ input, token, autoSubmit = false, onScan, onStatus }) {
      const el = typeof input === 'string' ? document.querySelector(input) : input;
      let ws, retry = 500;
      const open = () => {
        ws = new WebSocket(`wss://${base}/ws?role=desktop&t=${encodeURIComponent(token)}`);
        ws.onopen = () => { retry = 500; };
        ws.onmessage = (e) => {
          const m = JSON.parse(e.data);
          if (m.type === 'status') return onStatus && onStatus(m);
          if (m.type !== 'scan') return;
          el.value = m.code;
          el.dispatchEvent(new Event('input', { bubbles: true }));   // frameworks (Vue/React) pick this up
          el.dispatchEvent(new Event('change', { bubbles: true }));
          el.focus();
          if (autoSubmit) {
            if (el.form && el.form.requestSubmit) el.form.requestSubmit();
            else el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
          }
          onScan && onScan(m);
        };
        ws.onclose = () => { setTimeout(open, retry); retry = Math.min(retry * 2, 8000); };
      };
      open();
      return { close: () => { ws.onclose = null; ws.close(); } };
    },
  };
})();
