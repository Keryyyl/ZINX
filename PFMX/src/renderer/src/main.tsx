import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { api } from './bridge'
import App from './finance/FinanceApp'
import '@fontsource-variable/inter'
import '@fontsource-variable/geist'
import './styles.css'

// Komponenty znają tylko `window.api`, tak jak w ZIM
window.api = api

// Platforma na <html>: CSS paska przeciągania (.titlebar-drag) i odstępu paska bocznego działa tylko dla mac
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
