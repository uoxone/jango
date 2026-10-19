/////// A route result: a string or a Promise of a string ///////
export type Handler = () => string | Promise<string>

/////// A route is a function or an html file name ///////
export type RouteTarget = Handler | string

/////// A page kept in memory, or a function run on each request ///////
export type Route =
  | { kind: "static"; body: string }
  | { kind: "live"; run: Handler }

/////// Path to response ///////
export type Routes = Record<string, Route>

/////// One static file: content type and bytes ///////
export type StaticFile = {
  type: string
  body: Buffer
}

/////// URL to static file. Lookup is O(1) ///////
export type StaticFiles = Map<string, StaticFile>

/////// Host, port, static directory, and its URL ///////
export type Settings = {
  host: string
  port: number
  staticRoot: string
  staticUrl: string
}

/////// A project after it has been read from disk ///////
export type Project = {
  settings: Settings
  routes: Routes
  files: StaticFiles
}
