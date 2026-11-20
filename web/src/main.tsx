import { createRoot } from 'react-dom/client'
import { connectHost, createKitStrings } from '@kition/kit-editor'
// The editor's stylesheet first (its theme variables, Tailwind, and the `.data-*` classes), then
// kit-editor's, so the shell keeps its own look over Tailwind's reset.
import './styles/index.css'
import '@kition/kit-editor/styles.css'
import './app.css'
import { App } from './App'
import { CAPABILITIES } from './capabilities'
import { createTableI18n } from './i18n'
import { CATALOGS } from './strings'
import manifest from '../../kit.json'

const root = createRoot(document.getElementById('root')!)

// Opened outside a host, the page says where it belongs instead of waiting forever.
const alone = window.setTimeout(() => {
  const t = createKitStrings(CATALOGS, navigator.language)
  root.render(<div className="kui-alone">{t('noHost')}</div>)
}, 3000)

void connectHost({ id: 'kit.table', version: manifest.version, capabilities: CAPABILITIES, brand: manifest.ui.brand }).then(async (host) => {
  window.clearTimeout(alone)
  const i18n = await createTableI18n(host.init.locale)
  // No StrictMode: its doubled effects would open the document twice, in two engine workers.
  root.render(<App host={host} i18n={i18n} />)
})
