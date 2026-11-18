import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { AppProvider, useApp } from './app';
import { Login } from './shell/Login';
import { Shell } from './shell/Shell';
import './styles.css';

function Gate() {
  const { token } = useApp();
  return token ? <Shell /> : <Login />;
}

createRoot(document.getElementById('root') as HTMLElement).render(
  <StrictMode>
    <AppProvider><Gate /></AppProvider>
  </StrictMode>,
);
