import { mkdir, writeFile } from "node:fs/promises"
import { resolve } from "node:path"

const NAME = /^[A-Za-z][A-Za-z0-9_-]*$/

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
  ])

  console.log(`Created ${name}`)
  console.log(`\n  dingo run ${name}`)
}

function packageJson(name: string) {
  return `${JSON.stringify({ name: name.toLowerCase(), private: true, type: "module" }, null, 2)}\n`
}

function settingsSource() {
  return `const settings = {
  host: "127.0.0.1",
  port: 8000,
}

export default settings
`
}

function urlSource() {
  return `const url = {}

export default url
`
}

function isExists(error: unknown) {
  return typeof error === "object" && error !== null && "code" in error && error.code === "EEXIST"
}
