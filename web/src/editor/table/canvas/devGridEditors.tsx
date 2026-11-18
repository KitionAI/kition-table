/**
 * The development page's popup editors (`dev/grid.html`): the grid's date and choice editors
 * (`GridCellEditors.tsx`) in a React root of their own, with the editor's strings, as the editor
 * will hold them in its own tree.
 */
import { createRoot } from 'react-dom/client'
import { I18nextProvider } from 'react-i18next'
import { createTableI18n } from '../../../i18n'
import { GridCellPopup } from './GridCellEditors'
import type { CellPopup } from './gridEditing'

/** Mounts the popups in `host`; answers how to show one (null closes it). */
export async function mountPopups(host: HTMLElement, locale = 'en-US'): Promise<(popup: CellPopup | null) => void> {
  const i18n = await createTableI18n(locale)
  const root = createRoot(host)
  return (popup) =>
    root.render(
      <I18nextProvider i18n={i18n}>
        <GridCellPopup popup={popup} locale={locale} />
      </I18nextProvider>,
    )
}
