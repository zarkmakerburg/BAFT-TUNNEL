/// <reference types="vite/client" />
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { AppProvider, useApp } from './app';
import { Login } from './shell/Login';
import { Shell } from './shell/Shell';
import './styles.css';

function Gate() {
  const { token } = useApp();
  // Local-only visual preview: never included in production builds and never grants API access.
  const localPreview = import.meta.env.DEV && new URLSearchParams(window.location.search).get('preview') === '1';
  return token || localPreview ? <Shell /> : <Login />;
}

createRoot(document.getElementById('root') as HTMLElement).render(
  <StrictMode>
    <AppProvider><Gate /></AppProvider>
  </StrictMode>,
);
