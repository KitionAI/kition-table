import { useEffect, useRef } from 'react'
import { thumbnail } from '../canvas/engineSource'
import { blobHashOfURL } from '../lib/blobAttachments'
import { useTableDataSource } from '../ports/TableDataSource'
import { resolveTableFileURL, useTableHost } from '../ports/TableHost'

/** The longest side of a cover's picture, in pixels: a card's cover at twice its size. */
const COVER = 768

/**
 * A gallery card's cover picture. A picture the document holds (a blob, or a `data:` url) is decoded as the grid
 * decodes its thumbnails (`thumbnail`: the browser's decoder, else the engine's `gridPicture`), so a GIF shows its
 * first frame and an SVG or a HEIC shows in every browser, as in the grid and the headless picture; it is drawn into a
 * canvas the size of the decoded picture, which the stylesheet covers the box with. A picture at a web address is an
 * image the host resolves, as the grid's own loader shows it.
 */
export function GalleryCover({ url, documentId, label }: { url: string; documentId: number | null; label: string }) {
  const dataSource = useTableDataSource()
  const host = useTableHost()
  const canvas = useRef<HTMLCanvasElement>(null)
  const hash = blobHashOfURL(url)
  const read = dataSource.attachments?.read
  const decode = dataSource.grid.picture
  const held = (hash != null && read != null && documentId != null) || url.startsWith('data:')

  useEffect(() => {
    if (!held) return
    let live = true
    const load = async () => {
      const blob =
        hash != null && read && documentId != null
          ? await read({ documentId, hash }).then((file) => new Blob([file.data as BlobPart], { type: file.mimeType }))
          : await (await fetch(url)).blob()
      const bitmap = await thumbnail(blob, 0, 0, decode, COVER)
      const target = canvas.current
      if (!bitmap) return
      if (live && target) {
        target.width = bitmap.width
        target.height = bitmap.height
        target.getContext('2d')?.drawImage(bitmap, 0, 0)
        target.dataset.ktableGalleryCover = 'drawn'
      }
      bitmap.close()
    }
    load().catch(() => undefined)
    return () => {
      live = false
    }
  }, [held, hash, read, decode, documentId, url])

  if (!held) return <img src={resolveTableFileURL(host, url)} alt={label} loading="lazy" />
  return <canvas ref={canvas} role="img" aria-label={label} data-ktable-gallery-cover="loading" />
}
