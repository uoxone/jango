import { createServer, type ServerResponse } from "node:http"
import { availableParallelism } from "node:os"
import { Worker } from "node:worker_threads"
import { loadProject } from "./load.ts"
import type { Handler, Project, Routes, Settings, StaticFiles } from "./types.ts"

/////// Fixed responses are built once so a request does not allocate them again ///////
const HTML = "text/html; charset=utf-8"
const TEXT = "text/plain; charset=utf-8"
const NOT_FOUND = Buffer.from("Not Found")
const SERVER_ERROR = Buffer.from("Internal Server Error")
const NOT_FOUND_HEADERS = { "content-type": TEXT, "content-length": NOT_FOUND.length }
const SERVER_ERROR_HEADERS = { "content-type": TEXT, "content-length": SERVER_ERROR.length }

/////// A ready page or file: headers and body in memory ///////
type StaticHit = {
  headers: { "content-type": string; "content-length": number }
  body: Buffer
}

/////// The Bun server shape, without depending on Bun's own types ///////
type BunServer = {
  port: number
  stop(closeActiveConnections?: boolean): void
}

type BunRuntime = {
  serve(options: {
    hostname: string
    port: number
    reusePort?: boolean
    development?: boolean
    routes?: Record<string, Response>
    fetch: (req: Request) => Response | Promise<Response>
  }): BunServer
}

/////// The running server address and its close function ///////
export type Running = {
  url: string
  close(): Promise<void>
}

/////// reusePort shares the port across processes. quiet hides the startup line ///////
type ServeOptions = {
  reusePort?: boolean
  quiet?: boolean
}

/////// Loads the project and listens once per CPU core ///////
export async function serveProject(dir: string): Promise<Running> {
  const project = await loadProject(dir)
  const count = workerCount(project.settings.port)
  if (count === 1) return serve(project)

  const running = await serve(project, { reusePort: true })
  const children = Array.from({ length: count - 1 }, () =>
    new Worker(new URL("./worker.ts", import.meta.url), { workerData: { dir } }),
  )
  for (const child of children) {
    child.on("error", (error) => console.error(error))
  }

  return {
    url: running.url,
    async close() {
      await Promise.all(children.map((child) => child.terminate()))
      await running.close()
    },
  }
}

/////// Bun serves prepared routes. Node serves a memory map ///////
export async function serve(project: Project, options: ServeOptions = {}): Promise<Running> {
  const compiled = compile(project.routes, project.files)
  const bun = (globalThis as { Bun?: BunRuntime }).Bun

  if (bun) return serveBun(bun, project.settings, compiled, options)
  return serveNode(project.settings, compiled, options)
}

/////// Builds responses before the first request. A url.ts route wins over a static file ///////
function compile(routes: Routes, files: StaticFiles) {
  const bunRoutes: Record<string, Response> = {}
  const nodeStatic = new Map<string, StaticHit>()
  const live = new Map<string, Handler>()
  const bodies = new Map<string, string>()
  const onBun = "Bun" in globalThis

  for (const [path, route] of Object.entries(routes)) {
    if (route.kind === "live") {
      live.set(path, route.run)
      continue
    }

    if (onBun) {
      bodies.set(path, route.body)
      bunRoutes[path] = new Response(route.body, {
        headers: {
          "content-type": HTML,
          "content-length": String(Buffer.byteLength(route.body)),
        },
      })
      continue
    }

    const body = Buffer.from(route.body)
    nodeStatic.set(path, {
      headers: { "content-type": HTML, "content-length": body.length },
      body,
    })
  }

  for (const [path, file] of files) {
    if (live.has(path) || bodies.has(path) || nodeStatic.has(path)) continue
    if (onBun) {
      bunRoutes[path] = new Response(new Uint8Array(file.body), {
        headers: {
          "content-type": file.type,
          "content-length": String(file.body.length),
        },
      })
      continue
    }

    nodeStatic.set(path, {
      headers: { "content-type": file.type, "content-length": file.body.length },
      body: file.body,
    })
  }

  return { bunRoutes, nodeStatic, live, bodies }
}

/////// Bun answers static routes without entering JavaScript ///////
function serveBun(
  bun: BunRuntime,
  settings: Settings,
  compiled: ReturnType<typeof compile>,
  options: ServeOptions,
): Running {
  let server: BunServer
  try {
    const fetch = (req: Request) => dispatch(compiled, bunPath(req.url))
    server = bun.serve(
      Object.keys(compiled.bunRoutes).length > 0
        ? {
            hostname: settings.host,
            port: settings.port,
            reusePort: options.reusePort,
            development: false,
            routes: compiled.bunRoutes,
            fetch,
          }
        : {
            hostname: settings.host,
            port: settings.port,
            reusePort: options.reusePort,
            development: false,
            fetch,
          },
    )
  } catch (error) {
    throw busyPort(error, settings.port)
  }

  if (!options.quiet) started("bun", settings.host, server.port)
  return {
    url: `http://${settings.host}:${server.port}`,
    async close() {
      server.stop(true)
    },
  }
}

/////// Each request is one Map lookup. The disk is not read ///////
async function serveNode(
  settings: Settings,
  compiled: ReturnType<typeof compile>,
  options: ServeOptions,
): Promise<Running> {
  const server = createServer((req, res) => {
    const path = nodePath(req.url ?? "/")
    const hit = compiled.nodeStatic.get(path)
    if (hit) {
      res.writeHead(200, hit.headers)
      res.end(hit.body)
      return
    }

    const run = compiled.live.get(path)
    if (!run) {
      res.writeHead(404, NOT_FOUND_HEADERS)
      res.end(NOT_FOUND)
      return
    }

    let result: string | Promise<string>
    try {
      result = run()
    } catch {
      res.writeHead(500, SERVER_ERROR_HEADERS)
      res.end(SERVER_ERROR)
      return
    }

    if (typeof result === "string") {
      send(res, 200, result)
      return
    }

    result.then(
      (body) => send(res, 200, body),
      () => {
        res.writeHead(500, SERVER_ERROR_HEADERS)
        res.end(SERVER_ERROR)
      },
    )
  })

  server.keepAliveTimeout = 60_000
  server.headersTimeout = 65_000
  server.on("connection", (socket) => {
    socket.setNoDelay(true)
  })

  await new Promise<void>((resolve, reject) => {
    const onError = (error: Error) => reject(busyPort(error, settings.port))
    server.once("error", onError)
    server.listen({ port: settings.port, host: settings.host, reusePort: options.reusePort }, () => {
      server.off("error", onError)
      resolve()
    })
  })

  const address = server.address()
  const port = typeof address === "object" && address ? address.port : settings.port
  if (!options.quiet) started("node", settings.host, port)

  return {
    url: `http://${settings.host}:${port}`,
    close: () =>
      new Promise((resolve, reject) => {
        server.closeAllConnections()
        server.close((error) => (error ? reject(error) : resolve()))
      }),
  }
}

/////// Returns a static route from memory and runs a function route ///////
function dispatch(compiled: ReturnType<typeof compile>, path: string) {
  const body = compiled.bodies.get(path)
  if (body) {
    return new Response(body, {
      headers: { "content-type": HTML, "content-length": String(Buffer.byteLength(body)) },
    })
  }

  const run = compiled.live.get(path)
  if (!run) return new Response("Not Found", { status: 404, headers: { "content-type": TEXT } })

  let result: string | Promise<string>
  try {
    result = run()
  } catch {
    return new Response("Internal Server Error", { status: 500, headers: { "content-type": TEXT } })
  }

  if (typeof result === "string") {
    return new Response(result, { headers: { "content-type": HTML } })
  }

  return result.then(
    (html) => new Response(html, { headers: { "content-type": HTML } }),
    () => new Response("Internal Server Error", { status: 500, headers: { "content-type": TEXT } }),
  )
}

/////// Sends an html body with its exact length ///////
function send(res: ServerResponse, status: number, body: string) {
  res.writeHead(status, {
    "content-type": status === 200 ? HTML : TEXT,
    "content-length": Buffer.byteLength(body),
  })
  res.end(body)
}

/////// Returns the Node request path without the query string ///////
function nodePath(url: string) {
  return requestPath(url)
}

/////// Takes the path out of Bun's full URL ///////
function bunPath(url: string) {
  const scheme = url.indexOf("://")
  const slash = scheme === -1 ? 0 : url.indexOf("/", scheme + 3)
  return requestPath(slash === -1 ? "/" : url.slice(slash))
}

/////// Drops the query string and decodes percent-encoded characters ///////
function requestPath(url: string) {
  const query = url.indexOf("?")
  const path = query === -1 ? url : url.slice(0, query)
  try {
    return decodeURIComponent(path)
  } catch {
    return path
  }
}

/////// Port 0 stays on one listener so tests do not collide ///////
function workerCount(port: number) {
  if (port === 0) return 1
  const requested = process.env.JANGO_WORKERS
  if (requested === undefined || requested === "") return availableParallelism()
  const count = Number(requested)
  if (!Number.isInteger(count) || count < 1) return 1
  return count
}

/////// One startup line: runtime and address ///////
function started(runtime: "bun" | "node", host: string, port: number) {
  console.log(`Jango  ${runtime}  http://${host}:${port}`)
}

/////// Turns a busy port into a message that points at settings.ts ///////
function busyPort(error: unknown, port: number) {
  const message = error instanceof Error ? error.message : String(error)
  if (message.includes("EADDRINUSE") || /port \d+ in use/i.test(message)) {
    return new Error(`Port ${port} is already in use.\nStop the other server, or change port in settings.ts.`)
  }
  return error instanceof Error ? error : new Error(message)
}
