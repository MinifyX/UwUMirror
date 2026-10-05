import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import './components/nyu/nyu.css';
import { applyAppearance } from './lib/settings';
import './styles/tokens.css';
import './styles/app.css';
import './styles/home.css';
import './styles/stream.css';

// Light or dark as the system is, like UwUMail; Settings → General picks one,
// and decides about animations.
applyAppearance();

const root = document.getElementById('root');
if (!root) throw new Error('#root missing from index.html');

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
