import { workerData } from "node:worker_threads"
import { loadProject } from "./load.ts"
import { serve } from "./server.ts"

const { dir } = workerData as { dir: string }
const project = await loadProject(dir)
await serve(project.settings, project.routes, { reusePort: true, quiet: true })
