export type Handler = () => string | Promise<string>

export type RouteTarget = Handler | string

export type Route =
  | { kind: "static"; body: string }
  | { kind: "live"; run: Handler }

export type Routes = Record<string, Route>

export type Settings = {
  host: string
  port: number
}
