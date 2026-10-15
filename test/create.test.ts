import assert from "node:assert/strict"
import { access, readFile } from "node:fs/promises"
import { join } from "node:path"
import { describe, it } from "node:test"
import { createProject } from "../src/create.ts"
import { tempDir } from "./fixture.ts"

describe("create project", () => {
  it("writes settings, urls, and package.json", async () => {
    const workspace = await tempDir()
    try {
      await createProject("Shop", workspace.dir)
      const root = join(workspace.dir, "Shop")
      const settings = await readFile(join(root, "settings.ts"), "utf8")
      const url = await readFile(join(root, "url.ts"), "utf8")
      const pkg = JSON.parse(await readFile(join(root, "package.json"), "utf8"))

      assert.match(settings, /port: 8000/)
      assert.match(url, /const url = \{\}/)
      assert.equal(pkg.name, "shop")
      assert.equal(pkg.type, "module")
      await assert.rejects(access(join(root, "jango.html")))
    } finally {
      await workspace.remove()
    }
  })

  it("rejects a second project in the same folder", async () => {
    const workspace = await tempDir()
    try {
      await createProject("shop", workspace.dir)
      await assert.rejects(createProject("shop", workspace.dir), /Project already exists: shop/)
    } finally {
      await workspace.remove()
    }
  })

  it("rejects names that are not a single identifier", async () => {
    const workspace = await tempDir()
    try {
      await assert.rejects(createProject("1shop", workspace.dir), /Project name/)
      await assert.rejects(createProject("my/shop", workspace.dir), /Project name/)
      await assert.rejects(createProject("a".repeat(65), workspace.dir), /Project name/)
    } finally {
      await workspace.remove()
    }
  })
})
