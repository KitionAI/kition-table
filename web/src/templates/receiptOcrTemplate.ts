import type { TFunction } from 'i18next'

import receiptSamples from './receiptSamples.json'
import type { TableTemplateSeed } from './tableTemplates'

/** A sample receipt: its fictional store, items and totals. `web/scripts/render-receipts.mjs` draws
 * the template's receipt pictures from the same data, so the seeded fields match the pictures. */
export interface ReceiptSample {
  record: number
  fileName: string
  vendor: string
  addressLines: string[]
  category: string
  currency: string
  symbol: string
  date: string
  time: string
  payment: string
  items: Array<{ description: string, quantity: number, unitPrice: number }>
  subtotal: number
  tax: number
  taxLabel: string
  taxIncluded: boolean
  total: number
}

export const RECEIPT_SAMPLES: ReceiptSample[] = receiptSamples

const money = (sample: ReceiptSample, value: number) => `${sample.symbol}${value.toFixed(2)}`

/** The lines a receipt shows, in reading order (the Plain Text field). */
export function receiptLines(sample: ReceiptSample): string[] {
  return [
    sample.vendor,
    ...sample.addressLines,
    [sample.date, sample.time].filter(Boolean).join(' '),
    ...sample.items.map((item) => `${item.description} ${money(sample, item.quantity * item.unitPrice)}`),
    `Subtotal ${money(sample, sample.subtotal)}`,
    `${sample.taxLabel} ${money(sample, sample.tax)}`,
    `TOTAL ${money(sample, sample.total)}`,
    sample.payment,
  ]
}

function receiptRecord(sample: ReceiptSample) {
  const address = sample.addressLines.join(', ')
  return {
    'File Name': sample.fileName,
    'Receipt Image': receiptAsset(sample.record),
    'Vendor Name': sample.vendor,
    Address: address,
    Category: sample.category,
    'Structured Data': JSON.stringify({
      merchant_name: sample.vendor,
      address,
      receipt_date: sample.date,
      currency: sample.currency,
      subtotal: sample.subtotal,
      tax: sample.tax,
      total: sample.total,
      payment_method: sample.payment,
      line_items: sample.items.map((item) => ({
        description: item.description,
        quantity: item.quantity,
        unit_price: item.unitPrice,
        amount: Number((item.quantity * item.unitPrice).toFixed(2)),
      })),
    }),
    'Plain Text': receiptLines(sample).join('\n'),
  }
}

function receiptAsset(recordNumber: number) {
  const recordID = `record-${String(recordNumber).padStart(2, '0')}`
  return { assetIds: [`${recordID}-receipt-image`] }
}

const VENDOR_SCHEMA = JSON.stringify({
  type: 'string',
  description: 'The merchant or vendor name only. Preserve the official capitalization visible on the receipt.',
})

const ADDRESS_SCHEMA = JSON.stringify({
  type: 'string',
  description: 'The merchant street address only. Include city, region, postal code, and country when visible.',
})

const STRUCTURED_RECEIPT_SCHEMA = JSON.stringify({
  merchant_name: 'string or null',
  address: 'string or null',
  receipt_date: 'ISO 8601 date string or null',
  currency: 'ISO 4217 currency code or null',
  subtotal: 'number or null',
  tax: 'number or null',
  total: 'number or null',
  payment_method: 'string or null',
  line_items: [{
    description: 'string',
    quantity: 'number or null',
    unit_price: 'number or null',
    amount: 'number or null',
  }],
})

const PLAIN_TEXT_SCHEMA = JSON.stringify({
  type: 'string',
  description: 'Every visible receipt line in reading order. Preserve line breaks and do not add commentary.',
})

export function createReceiptOcrTemplate(t: TFunction<'table'>): TableTemplateSeed {
  return {
    id: 'receipt-ocr-database',
    title: t('templateLibrary.templates.receiptOcrDatabase.title'),
    description: t('templateLibrary.templates.receiptOcrDatabase.description'),
    documentDescription: 'Upload receipt images and convert them into searchable fields, structured JSON, and plain OCR text.',
    icon: 'receipt-text',
    color: 'violet',
    assetManifestPath: 'templates/receipt-ocr-database/manifest.json',
    tables: [{
      title: 'Receipts',
      description: 'Receipt images with vision-based extraction for vendor details, categories, JSON, and plain text.',
      fields: [
        { title: 'File Name', type: 'text', primary: true },
        { title: 'Receipt Image', type: 'attachment' },
        {
          title: 'Vendor Name',
          type: 'text',
          aiConfig: {
            type: 'extract',
            sourceFieldTitle: 'Receipt Image',
            schema: VENDOR_SCHEMA,
            enabled: true,
            auto_update: true,
          },
        },
        {
          title: 'Address',
          type: 'text',
          aiConfig: {
            type: 'extract',
            sourceFieldTitle: 'Receipt Image',
            schema: ADDRESS_SCHEMA,
            enabled: true,
            auto_update: true,
          },
        },
        {
          title: 'Category',
          type: 'text',
          aiConfig: {
            type: 'classify',
            sourceFieldTitle: 'Receipt Image',
            categories: ['Food', 'Fuel', 'Software', 'Travel', 'Office supplies', 'Healthcare', 'Others'],
            enabled: true,
            auto_update: true,
          },
        },
        {
          title: 'Structured Data',
          type: 'long_text',
          aiConfig: {
            type: 'extract',
            sourceFieldTitle: 'Receipt Image',
            schema: STRUCTURED_RECEIPT_SCHEMA,
            enabled: true,
            auto_update: true,
          },
        },
        {
          title: 'Plain Text',
          type: 'long_text',
          aiConfig: {
            type: 'extract',
            sourceFieldTitle: 'Receipt Image',
            schema: PLAIN_TEXT_SCHEMA,
            enabled: true,
            auto_update: true,
          },
        },
      ],
      views: [{
        title: 'Overall',
        type: 'grid',
        config: { row_height: 'tall', frozen_column_count: 1 },
        fieldLayouts: [
          { fieldTitle: 'File Name', position: 0, width: 220, frozen: true },
          { fieldTitle: 'Receipt Image', position: 1, width: 160 },
          { fieldTitle: 'Vendor Name', position: 2, width: 210 },
          { fieldTitle: 'Address', position: 3, width: 260 },
          { fieldTitle: 'Category', position: 4, width: 150 },
          { fieldTitle: 'Structured Data', position: 5, width: 360 },
          { fieldTitle: 'Plain Text', position: 6, width: 360 },
        ],
      }],
      records: RECEIPT_SAMPLES.map(receiptRecord),
    }],
  }
}
