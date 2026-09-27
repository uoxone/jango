import assert from "node:assert/strict"
import { readFile } from "node:fs/promises"
import { describe, it } from "node:test"

describe("docs", () => {
  it("ships static html pages", async () => {
    for (const page of ["index.html", "start.html", "routes.html", "test.html"]) {
      const html = await readFile(new URL(`../docs/${page}`, import.meta.url), "utf8")
      assert.match(html, /lang="en"/)
      assert.doesNotMatch(html, /lang="fa"|dir="rtl"/)
      assert.match(html, /<main>/)
      assert.match(html, /docs\.css/)
    }
  })
})
