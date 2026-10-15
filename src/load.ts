import { readFile } from "node:fs/promises"
import { basename, resolve, sep } from "node:path"
import { pathToFileURL } from "node:url"
import type { Route, RouteTarget, Routes, Settings } from "./types.ts"

export async function loadProject(dir: string) {
  const root = resolve(dir)
  const settings = await readDefault<Settings>(root, "settings.ts")
  const declared = await readDefault<Record<string, RouteTarget>>(root, "url.ts")

  if (!settings || typeof settings.port !== "number" || typeof settings.host !== "string") {
    throw new Error("settings.ts must default-export { host, port }")
  }

  if (!declared || typeof declared !== "object") {
    throw new Error("url.ts must default-export routes")
  }

  const routes: Routes = {}
  for (const [path, route] of Object.entries(declared)) {
    if (!path.startsWith("/")) throw new Error(`Invalid route: ${path}`)
    routes[path] = await bindRoute(root, route)
  }

  if (!routes["/"]) routes["/"] = await welcome(basename(root))

  return { settings, routes }
}

async function welcome(name: string): Promise<Route> {
  const page = await readFile(new URL("./templates/jango.html", import.meta.url), "utf8")
  return { kind: "static", body: page.replaceAll("{{ name }}", name) }
}

async function bindRoute(root: string, route: RouteTarget): Promise<Route> {
  if (typeof route === "function") return { kind: "live", run: route }
  if (typeof route !== "string") throw new Error("A route must be a function or an .html file")

  const file = templatePath(root, route)
  try {
    return { kind: "static", body: await readFile(file, "utf8") }
  } catch (error) {
    if (isCode(error, "ENOENT")) throw new Error(`Missing ${route}`)
    throw error
  }
}

function templatePath(root: string, target: string) {
  if (!target.endsWith(".html") || target.startsWith("/") || target.includes("..") || target.includes("\0")) {
    throw new Error(`Invalid template: ${target}`)
  }

  const file = resolve(root, target)
  const base = root.endsWith(sep) ? root : root + sep
  if (!file.startsWith(base)) throw new Error(`Invalid template: ${target}`)
  return file
}

async function readDefault<T>(root: string, file: string): Promise<T> {
  try {
    const mod = await import(pathToFileURL(resolve(root, file)).href)
    return mod.default as T
  } catch (error) {
    if (isCode(error, "ERR_MODULE_NOT_FOUND")) {
      throw new Error(`Not a Jango project: missing ${file}`)
    }
    throw error
  }
}

function isCode(error: unknown, code: string) {
  return typeof error === "object" && error !== null && "code" in error && error.code === code
}
