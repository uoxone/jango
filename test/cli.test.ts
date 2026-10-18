import assert from "node:assert/strict"
import { execFile } from "node:child_process"
import { readFile } from "node:fs/promises"
import { describe, it } from "node:test"
import { fileURLToPath } from "node:url"
import { promisify } from "node:util"

const exec = promisify(execFile)
const root = new URL("..", import.meta.url)

describe("cli", () => {
  it("prints the package version", async () => {
    const { version } = JSON.parse(await readFile(new URL("package.json", root), "utf8"))
    for (const flag of ["-v", "--version"]) {
      const { stdout } = await exec("node", ["src/cli.ts", flag], { cwd: fileURLToPath(root) })
      assert.equal(stdout, `${version}\n`)
    }
  })
})
