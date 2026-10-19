import { mkdir, mkdtemp, rm } from "node:fs/promises"
import { join } from "node:path"
import { fileURLToPath } from "node:url"
import { createProject } from "../src/create.ts"
import { loadProject } from "../src/load.ts"
import { serve, type Running } from "../src/server.ts"

const scratch = fileURLToPath(new URL("./.tmp/", import.meta.url))

export async function tempDir() {
  await mkdir(scratch, { recursive: true })
  const dir = await mkdtemp(join(scratch, "p-"))
  return {
    dir,
    remove: () => rm(dir, { recursive: true, force: true }),
  }
}

export async function project(name = "shop") {
  const workspace = await tempDir()
  await createProject(name, workspace.dir)
  return {
    dir: join(workspace.dir, name),
    remove: workspace.remove,
  }
}

export async function listen(dir: string): Promise<Running> {
  const loaded = await loadProject(dir)
  loaded.settings.port = 0
  return serve(loaded)
}
