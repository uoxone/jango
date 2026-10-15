import { createProject } from "./create.ts"
import { serveProject } from "./server.ts"

const args = process.argv.slice(2)

try {
  await main()
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error))
  process.exit(1)
}

async function main() {
  const [command, noun, name] = args

  if (command === "create" && noun === "project" && name && args.length === 3) {
    await createProject(name)
    return
  }

  if (command === "run" && args.length <= 2) {
    await serveProject(noun ?? ".")
    return
  }

  console.error(`Usage:
  jango create project <name>
  jango run [dir]`)
  process.exitCode = 1
}
