// Installs the published @kition/sdk and @kition/tessera packages from npm. For local development
// against checkouts of kition-sdk and kition-tessera, set KITION_LINK=1: `pnpm install` then links
// kition-sdk's packages/sdk and kition-tessera's packages/tessera from beside this repository (or
// from KITION_SDK and KITION_TESSERA) instead. Both packages' `exports` point at their built
// `dist/`, so build them in the checkouts first (`pnpm --filter @kition/sdk build` in kition-sdk,
// `pnpm build` in kition-tessera's packages/tessera). Do not commit the lockfile a linked install writes.
const fs = require('node:fs')
const path = require('node:path')

const repo = path.resolve(__dirname, '..')
const sdk = path.resolve(process.env.KITION_SDK ?? path.join(repo, '..', 'kition-sdk'))
const tessera = path.resolve(process.env.KITION_TESSERA ?? path.join(repo, '..', 'kition-tessera'))

const local = {
  '@kition/sdk': path.join(sdk, 'packages/sdk'),
  '@kition/tessera': path.join(tessera, 'packages/tessera'),
}

function readPackage(pkg) {
  if (pkg.name !== 'kition-table-ui' || process.env.KITION_LINK !== '1') return pkg
  for (const field of ['dependencies', 'devDependencies']) {
    for (const name of Object.keys(pkg[field] ?? {})) {
      const dir = local[name]
      if (dir && fs.existsSync(path.join(dir, 'package.json'))) {
        pkg[field][name] = `link:${path.relative(__dirname, dir).split(path.sep).join('/')}`
      }
    }
  }
  return pkg
}

module.exports = { hooks: { readPackage } }
