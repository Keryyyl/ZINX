import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { api } from './bridge'
import App from './App'
import '@fontsource-variable/inter'
import '@fontsource-variable/geist'
import './styles.css'
import './app.css'

// Reszta interfejsu zna tylko `window.api`
window.api = api

// Platforma na <html>: pasek przeciągania i odstęp paska bocznego działają tylko dla mac
const ua = navigator.userAgent
document.documentElement.dataset.platform = /Android/.test(ua)
  ? 'android'
  : /iPhone|iPad/.test(ua)
    ? 'ios'
    : /Mac/.test(ua)
      ? 'mac'
      : 'other'

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>
)
