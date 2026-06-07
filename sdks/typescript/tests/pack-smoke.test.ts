// Packaging smoke test. DEPENDS ON a prior `npm run build` — CI (and the
// `test` script's local use) runs build before test, so dist/ is fresh. This is
// a thin "exports-map targets exist" guard; the authoritative NodeNext-tarball
// check is the CI `npm pack --dry-run` step (publish/ci-typescript workflows).
import { describe, it, expect } from 'vitest'
import { existsSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'

const packageRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..')

describe('packaging smoke', () => {
  it('exports-map targets exist after build', () => {
    expect(existsSync(resolve(packageRoot, 'dist/index.js'))).toBe(true)
    expect(existsSync(resolve(packageRoot, 'dist/index.d.ts'))).toBe(true)
  })
})
