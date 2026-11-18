// Installs the published @kition/* packages from npm. For local development against checkouts of
// kition-sdk and kition-tessera, set KITION_LINK=1: `pnpm install` then links their packages from
// beside this repository (or from KITION_SDK and KITION_TESSERA) instead.
const fs = require('node:fs')
const path = require('node:path')

const repo = path.resolve(__dirname, '..')
const sdk = path.resolve(process.env.KITION_SDK ?? path.join(repo, '..', 'kition-sdk'))
const tessera = path.resolve(process.env.KITION_TESSERA ?? path.join(repo, '..', 'kition-tessera'))

const local = {
  '@kition/kit-editor': path.join(sdk, 'packages/kit-editor'),
  '@kition/kit-host': path.join(sdk, 'packages/kit-host'),
  '@kition/kit-ui-protocol': path.join(sdk, 'packages/kit-ui-protocol'),
  '@kition/kit-dev-host': path.join(sdk, 'packages/kit-dev-host'),
  '@kition/tessera-icons': path.join(tessera, 'packages/icons'),
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
