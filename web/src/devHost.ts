/** The development host (dev.html): opens `public/sample/Sample.ktable` (scripts/sample.mjs), or a file chosen in the page. */
import { startDevHost } from '@kition/kit-dev-host'

startDevHost({ sample: './sample/Sample.ktable' })
