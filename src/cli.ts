import { readFile } from "node:fs/promises"
import { createProject } from "./create.ts"
import { serveProject } from "./server.ts"

/////// Arguments after the command name ///////
const args = process.argv.slice(2)

try {
  await main()
} catch (error) {
  console.error(error instanceof Error ? error.message : String(error))
  process.exit(1)
}

/////// Runs create, run, and version. An unknown command prints the usage ///////
async function main() {
  const [command, noun, name] = args

  if ((command === "-v" || command === "--version") && args.length === 1) {
    console.log(await version())
    return
  }

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
  jango run [dir]
  jango -v`)
  process.exitCode = 1
}

/////// Reads the version from this package's package.json ///////
async function version() {
  const { version } = JSON.parse(await readFile(new URL("../package.json", import.meta.url), "utf8"))
  return version
}
