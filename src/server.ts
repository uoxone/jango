import { createServer, type ServerResponse } from "node:http"
import { availableParallelism } from "node:os"
import { Worker } from "node:worker_threads"
import { loadProject } from "./load.ts"
import type { Handler, Routes, Settings } from "./types.ts"

const HTML = "text/html; charset=utf-8"
const TEXT = "text/plain; charset=utf-8"
const NOT_FOUND = Buffer.from("Not Found")
const SERVER_ERROR = Buffer.from("Internal Server Error")
const NOT_FOUND_HEADERS = { "content-type": TEXT, "content-length": NOT_FOUND.length }
const SERVER_ERROR_HEADERS = { "content-type": TEXT, "content-length": SERVER_ERROR.length }

type StaticHit = {
  headers: { "content-type": string; "content-length": number }
  body: Buffer
}

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

export type Running = {
  url: string
  close(): Promise<void>
}

type ServeOptions = {
  reusePort?: boolean
  quiet?: boolean
}

export async function serveProject(dir: string): Promise<Running> {
  const project = await loadProject(dir)
  const count = workerCount(project.settings.port)
  if (count === 1) return serve(project.settings, project.routes)

  const running = await serve(project.settings, project.routes, { reusePort: true })
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

export async function serve(settings: Settings, routes: Routes, options: ServeOptions = {}): Promise<Running> {
  const compiled = compile(routes)
  const bun = (globalThis as { Bun?: BunRuntime }).Bun

  if (bun) return serveBun(bun, settings, compiled, options)
  return serveNode(settings, compiled, options)
}

function compile(routes: Routes) {
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

  return { bunRoutes, nodeStatic, live, bodies }
}

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

function send(res: ServerResponse, status: number, body: string) {
  res.writeHead(status, {
    "content-type": status === 200 ? HTML : TEXT,
    "content-length": Buffer.byteLength(body),
  })
  res.end(body)
}

function nodePath(url: string) {
  const query = url.indexOf("?")
  return query === -1 ? url : url.slice(0, query)
}

function bunPath(url: string) {
  const scheme = url.indexOf("://")
  const slash = scheme === -1 ? 0 : url.indexOf("/", scheme + 3)
  const path = slash === -1 ? "/" : url.slice(slash)
  const query = path.indexOf("?")
  return query === -1 ? path : path.slice(0, query)
}

function workerCount(port: number) {
  if (port === 0) return 1
  const requested = process.env.JANGO_WORKERS
  if (requested === undefined || requested === "") return availableParallelism()
  const count = Number(requested)
  if (!Number.isInteger(count) || count < 1) return 1
  return count
}

function started(runtime: "bun" | "node", host: string, port: number) {
  console.log(`Jango  ${runtime}  http://${host}:${port}`)
}

function busyPort(error: unknown, port: number) {
  const message = error instanceof Error ? error.message : String(error)
  if (message.includes("EADDRINUSE") || /port \d+ in use/i.test(message)) {
    return new Error(`Port ${port} is already in use.\nStop the other server, or change port in settings.ts.`)
  }
  return error instanceof Error ? error : new Error(message)
}
