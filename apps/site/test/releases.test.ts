import { describe, expect, it } from 'bun:test'

// The generator is plain Node (.mjs); importing it must not run the CLI.
import {
  channelFromTag,
  classifyAsset,
  isIgnoredAsset,
} from '../tools/generate-release-notes.mjs'
import {
  compareReleasesDesc,
  releaseRank,
} from '../src/content/releaseOrdering'
import type { ReleaseEntry } from '../src/content/types'

describe('channelFromTag', () => {
  it('maps plain semver to stable', () => {
    expect(channelFromTag('v0.2.0')).toBe('stable')
    expect(channelFromTag('1.2.3')).toBe('stable')
  })

  it('maps nightly serials to nightly', () => {
    expect(channelFromTag('v0.2.0-nightly.44')).toBe('nightly')
  })

  it('excludes rolling pointers, RC, and dogfood', () => {
    expect(channelFromTag('nightly')).toBeNull()
    expect(channelFromTag('stable')).toBeNull()
    expect(channelFromTag('v0.2.0-rc.1')).toBeNull()
    expect(channelFromTag('v0.1.0-dogfood.39')).toBeNull()
  })
})

describe('classifyAsset', () => {
  // Real artifact names from a nightly release — the contract with release.yml.
  it('classifies desktop installers (stable + nightly naming)', () => {
    expect(
      classifyAsset('PosthasteNightly_0.2.0-nightly.44_aarch64.dmg'),
    ).toEqual({
      product: 'desktop',
      os: 'macOS',
      arch: 'Apple Silicon',
      kind: 'dmg',
    })
    expect(classifyAsset('Posthaste_0.2.0_aarch64.dmg')).toMatchObject({
      product: 'desktop',
      os: 'macOS',
    })
    expect(
      classifyAsset('PosthasteNightly_0.2.0-nightly.44_x64-setup.exe'),
    ).toMatchObject({ product: 'desktop', os: 'Windows', kind: 'exe' })
    expect(
      classifyAsset('PosthasteNightly_0.2.0-nightly.44_amd64.AppImage'),
    ).toMatchObject({ product: 'desktop', os: 'Linux', kind: 'AppImage' })
  })

  it('classifies the CLI per os/arch', () => {
    expect(classifyAsset('PosthasteCTLNightly-darwin-arm64')).toEqual({
      product: 'cli',
      os: 'macOS',
      arch: 'arm64',
      kind: 'binary',
    })
    expect(classifyAsset('PosthasteCTLNightly-windows-x64.exe')).toMatchObject({
      product: 'cli',
      os: 'Windows',
      arch: 'x64',
    })
    expect(classifyAsset('PosthasteCTL-linux-x64')).toMatchObject({
      product: 'cli',
      os: 'Linux',
    })
  })

  it('classifies the self-host daemon (incl. the arch-less macOS bundle)', () => {
    expect(classifyAsset('PosthasteDaemonNightly-linux-x86_64.tar.gz')).toEqual(
      { product: 'daemon', os: 'Linux', arch: 'x86_64', kind: 'tar.gz' },
    )
    expect(classifyAsset('PosthasteDaemonNightly-macos.tar.gz')).toEqual({
      product: 'daemon',
      os: 'macOS',
      arch: '',
      kind: 'tar.gz',
    })
  })

  it('drops signatures, checksums, updater + web artifacts', () => {
    for (const name of [
      'PosthasteNightly_0.2.0-nightly.44_aarch64.dmg.sig',
      'PosthasteCTLNightly-darwin-arm64.sigstore.json',
      'SHA256SUMS',
      'latest.json',
      'MACOS-INSTALL.txt',
      'PosthasteNightly.app.tar.gz',
      'PosthasteWebNightly.tar.gz',
    ]) {
      expect(isIgnoredAsset(name)).toBe(true)
      expect(classifyAsset(name)).toBeNull()
    }
  })

  it('flags a genuinely unrecognised artifact (drift alarm)', () => {
    // Not ignored and not classified -> the generator warns instead of silently dropping it.
    expect(isIgnoredAsset('PosthasteRunner-linux-x64.zip')).toBe(false)
    expect(classifyAsset('PosthasteRunner-linux-x64.zip')).toBeNull()
  })
})

describe('release ordering', () => {
  it('ranks nightly serials and puts stable above its own nightlies', () => {
    expect(releaseRank('v0.2.0')).toEqual([0, 2, 0, Number.POSITIVE_INFINITY])
    expect(releaseRank('v0.2.0-nightly.44')).toEqual([0, 2, 0, 44])
  })

  const entry = (tag: string): ReleaseEntry =>
    ({ tag }) as unknown as ReleaseEntry

  it('sorts newest-first across versions, serials, and channels', () => {
    const sorted = [
      entry('v0.2.0-nightly.8'),
      entry('v0.2.0'),
      entry('v0.2.0-nightly.44'),
      entry('v0.1.99-nightly.3'),
      entry('v0.2.0-nightly.40'),
    ].sort(compareReleasesDesc)

    expect(sorted.map((e) => e.tag)).toEqual([
      'v0.2.0',
      'v0.2.0-nightly.44',
      'v0.2.0-nightly.40',
      'v0.2.0-nightly.8',
      'v0.1.99-nightly.3',
    ])
  })
})
