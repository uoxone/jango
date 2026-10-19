import { workerData } from "node:worker_threads"
import { loadProject } from "./load.ts"
import { serve } from "./server.ts"

/////// An extra process listens on the shared port and does not print the startup line ///////
const { dir } = workerData as { dir: string }
const project = await loadProject(dir)
await serve(project, { reusePort: true, quiet: true })
