import { readdir, readFile } from "node:fs/promises"
import { join, relative, resolve, sep } from "node:path"
import type { StaticFiles } from "./types.ts"

/////// Extension to Content-Type. Looked up once at startup ///////
const TYPES: Record<string, string> = {
  css: "text/css; charset=utf-8",
  js: "text/javascript; charset=utf-8",
  mjs: "text/javascript; charset=utf-8",
  json: "application/json; charset=utf-8",
  map: "application/json",
  svg: "image/svg+xml",
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  ico: "image/x-icon",
  woff: "font/woff",
  woff2: "font/woff2",
  ttf: "font/ttf",
  txt: "text/plain; charset=utf-8",
  html: "text/html; charset=utf-8",
  webmanifest: "application/manifest+json",
  pdf: "application/pdf",
  xml: "application/xml",
}

/////// Reads staticRoot once and indexes the files under staticUrl ///////
export async function loadStatic(projectRoot: string, staticRoot: string, staticUrl: string): Promise<StaticFiles> {
  const dir = staticDir(projectRoot, staticRoot)
  const prefix = staticPrefix(staticUrl)
  const files: StaticFiles = new Map()

  try {
    await walk(dir, dir, prefix, files)
  } catch (error) {
    if (isCode(error, "ENOENT")) return files
    if (isCode(error, "ENOTDIR")) throw new Error("staticRoot must be a directory")
    throw error
  }

  return files
}

/////// Reads a directory in parallel. Symbolic links are skipped so a path cannot leave the folder ///////
async function walk(dir: string, root: string, prefix: string, files: StaticFiles) {
  await Promise.all(
    (await readdir(dir, { withFileTypes: true })).map(async (entry) => {
      const path = join(dir, entry.name)
      if (entry.isDirectory()) {
        await walk(path, root, prefix, files)
        return
      }
      if (!entry.isFile()) return

      const name = relative(root, path).split(sep).join("/")
      files.set(`${prefix}${name}`, { type: contentType(entry.name), body: await readFile(path) })
    }),
  )
}

/////// staticRoot must be a directory inside the project ///////
function staticDir(projectRoot: string, staticRoot: string) {
  if (!isSafePath(staticRoot)) throw new Error(`Invalid staticRoot: ${staticRoot}`)
  const dir = resolve(projectRoot, staticRoot)
  const base = projectRoot.endsWith(sep) ? projectRoot : projectRoot + sep
  if (!dir.startsWith(base)) throw new Error(`Invalid staticRoot: ${staticRoot}`)
  return dir
}

/////// staticUrl must start and end with /, such as /static/ ///////
function staticPrefix(staticUrl: string) {
  if (staticUrl === "/") return staticUrl
  const path = staticUrl.startsWith("/") && staticUrl.endsWith("/") ? staticUrl.slice(1, -1) : ""
  if (!path || !isSafePath(path)) throw new Error(`Invalid staticUrl: ${staticUrl}`)
  return staticUrl
}

/////// Rejects ".", "..", and any character that is not allowed ///////
function isSafePath(path: string) {
  const parts = path.split("/")
  return parts.every((part) => part !== "." && part !== ".." && /^[\w.-]+$/.test(part))
}

/////// Returns the response type from the file extension ///////
function contentType(name: string) {
  const dot = name.lastIndexOf(".")
  if (dot === -1 || dot === name.length - 1) return "application/octet-stream"
  return TYPES[name.slice(dot + 1).toLowerCase()] ?? "application/octet-stream"
}

/////// Matches a system error code ///////
function isCode(error: unknown, code: string) {
  return typeof error === "object" && error !== null && "code" in error && error.code === code
}
