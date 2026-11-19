import { describe, expect, it } from 'vitest'
import { createTableI18n } from './i18n'

/**
 * The footer's counts agree with their number in every locale (i18next's plural keys, by the
 * locale's plural rules): one record is not "1 records". Russian and Chinese name the count as a
 * label ("Записей: 1") or take no plural, so their one string serves every number.
 */
const FOOTER: Record<string, { records: [string, string]; selected: [string, string] }> = {
  'en-US': { records: ['1 record', '2 records'], selected: ['1 selected', '2 selected'] },
  'es-ES': { records: ['1 registro', '2 registros'], selected: ['1 seleccionado', '2 seleccionados'] },
  'fr-FR': { records: ['1 enregistrement', '2 enregistrements'], selected: ['1 sélectionné', '2 sélectionnés'] },
  'pt-BR': { records: ['1 registro', '2 registros'], selected: ['1 selecionado', '2 selecionados'] },
  'ru-RU': { records: ['Записей: 1', 'Записей: 2'], selected: ['Выбрано: 1', 'Выбрано: 2'] },
  'zh-CN': { records: ['1 条记录', '2 条记录'], selected: ['已选 1 项', '已选 2 项'] },
}

describe('the footer counts', () => {
  for (const [locale, expected] of Object.entries(FOOTER)) {
    it(`agree with their number in ${locale}`, async () => {
      const i18n = await createTableI18n(locale)
      expect(i18n.language).toBe(locale)
      expect([1, 2].map((count) => i18n.t('footer.recordsCount', { count }))).toEqual(expected.records)
      expect([1, 2].map((count) => i18n.t('footer.selectedCount', { count }))).toEqual(expected.selected)
    })
  }
})
