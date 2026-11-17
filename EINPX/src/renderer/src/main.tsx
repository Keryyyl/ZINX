import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { api } from './bridge'
import App from './App'
import '@fontsource-variable/inter'
import '@fontsource-variable/geist'
import './einpx.css'

// Reszta interfejsu zna tylko `window.api`
window.api = api

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>
)
