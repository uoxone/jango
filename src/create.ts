import { mkdir, writeFile } from "node:fs/promises"
import { resolve } from "node:path"

/////// A project name starts with a letter, then letters, numbers, _, or -, and is at most 64 characters ///////
const NAME = /^[A-Za-z][A-Za-z0-9_-]*$/

/////// Creates the project folder and writes the starter files at the same time ///////
export async function createProject(name: string, cwd = process.cwd()) {
  if (!NAME.test(name) || name.length > 64) {
    throw new Error(
      "Project name must start with a letter and use only letters, numbers, _ or -",
    )
  }

  const root = resolve(cwd, name)

  try {
    await mkdir(root)
  } catch (error) {
    if (isExists(error)) throw new Error(`Project already exists: ${name}`)
    throw error
  }

  await Promise.all([
    writeFile(resolve(root, "package.json"), packageJson(name), "utf8"),
    writeFile(resolve(root, "settings.ts"), settingsSource(), "utf8"),
    writeFile(resolve(root, "url.ts"), urlSource(), "utf8"),
    mkdir(resolve(root, "templates")),
    mkdir(resolve(root, "static")),
  ])

  console.log(`Created ${name}`)
  console.log(`\n  jango run ${name}`)
}

/////// A module package.json so the project's TypeScript files can be imported ///////
function packageJson(name: string) {
  return `${JSON.stringify({ name: name.toLowerCase(), private: true, type: "module" }, null, 2)}\n`
}

/////// Default settings: localhost, port 8000, the static folder, and /static/ ///////
function settingsSource() {
  return `const settings = {
  host: "127.0.0.1",
  port: 8000,
  staticRoot: "static",
  staticUrl: "/static/",
}

export default settings
`
}

/////// An empty route table. The default page is shown until / is defined ///////
function urlSource() {
  return `const url = {}

export default url
`
}

/////// Tells an existing folder apart from any other error ///////
function isExists(error: unknown) {
  return typeof error === "object" && error !== null && "code" in error && error.code === "EEXIST"
}
