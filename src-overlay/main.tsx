import React from 'react';
import ReactDOM from 'react-dom/client';
import Overlay from './Overlay';
import './overlay.css';

// Overlay penceresinin kendi mini giriş noktası (§3.3 dizin düzeni: src-overlay/).
// Ana pencere React ağacından (src/) bağımsız; aynı asset protokolünden
// (url: "src-overlay/overlay.html") ayrı bir webview'e yüklenir.
ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <Overlay />
  </React.StrictMode>,
);