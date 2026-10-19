import { readFile } from "node:fs/promises"
import { basename, resolve, sep } from "node:path"
import { pathToFileURL } from "node:url"
import { loadStatic } from "./static.ts"
import type { Project, Route, RouteTarget, Routes, Settings } from "./types.ts"

/////// Reads settings, routes, and static files from disk at the same time ///////
export async function loadProject(dir: string): Promise<Project> {
  const root = resolve(dir)
  const [settings, declared] = await Promise.all([
    readDefault<Settings>(root, "settings.ts"),
    readDefault<Record<string, RouteTarget>>(root, "url.ts"),
  ])

  if (
    !settings ||
    typeof settings.port !== "number" ||
    typeof settings.host !== "string" ||
    typeof settings.staticRoot !== "string" ||
    typeof settings.staticUrl !== "string"
  ) {
    throw new Error("settings.ts must default-export { host, port, staticRoot, staticUrl }")
  }

  if (!declared || typeof declared !== "object") {
    throw new Error("url.ts must default-export routes")
  }

  const [routes, files] = await Promise.all([
    collectRoutes(root, declared),
    loadStatic(root, settings.staticRoot, settings.staticUrl),
  ])

  return { settings, routes, files }
}

/////// Binds every route to a function or an html file at the same time ///////
async function collectRoutes(root: string, declared: Record<string, RouteTarget>): Promise<Routes> {
  const routes: Routes = {}
  await Promise.all(
    Object.entries(declared).map(async ([path, route]) => {
      if (!path.startsWith("/")) throw new Error(`Invalid route: ${path}`)
      routes[path] = await bindRoute(root, route)
    }),
  )

  if (!routes["/"]) routes["/"] = await welcome(basename(root))
  return routes
}

/////// Fills the default page with the project folder name ///////
async function welcome(name: string): Promise<Route> {
  const page = await readFile(new URL("./templates/jango.html", import.meta.url), "utf8")
  return { kind: "static", body: page.replaceAll("{{ name }}", name) }
}

/////// A function stays live. An html file is read once and kept in memory ///////
async function bindRoute(root: string, route: RouteTarget): Promise<Route> {
  if (typeof route === "function") return { kind: "live", run: route }
  if (typeof route !== "string") throw new Error("A route must be a function or an .html file")

  const file = templatePath(root, route)
  try {
    return { kind: "static", body: await readFile(file, "utf8") }
  } catch (error) {
    if (isCode(error, "ENOENT")) throw new Error(`Missing template: ${route}`)
    throw error
  }
}

/////// A template name must stay inside templates and cannot escape with .. ///////
function templatePath(root: string, name: string) {
  if (!name.endsWith(".html") || name.startsWith("/") || name.includes("..") || name.includes("\\") || name.includes("\0")) {
    throw new Error(`Invalid template: ${name}`)
  }

  const dir = resolve(root, "templates")
  const file = resolve(dir, name)
  const base = dir.endsWith(sep) ? dir : dir + sep
  if (!file.startsWith(base)) throw new Error(`Invalid template: ${name}`)
  return file
}

/////// Imports the default export of settings.ts or url.ts ///////
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

/////// Matches a system error code ///////
function isCode(error: unknown, code: string) {
  return typeof error === "object" && error !== null && "code" in error && error.code === code
}
