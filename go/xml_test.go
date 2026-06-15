package xml

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"strings"
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
)

// specEntry represents one row of a TSV spec file.
type specEntry struct {
	File     string
	Line     int
	Name     string
	Input    string // Escape-decoded XML source.
	Expected string // Raw cell: JSON text, or "ERROR" / "ERROR:code".
	Opts     string // Raw JSON (may be empty).
}

// specDir returns the absolute path to the shared TSV spec directory.
func specDir() string {
	return filepath.Join("..", "test", "spec")
}

// loadSpec reads a TSV spec file into a slice of specEntry. Comment and
// blank lines are skipped. Escapes in the `input` column are decoded
// via unescapeInput; the `expected` and `opts` columns are left raw so
// JSON's own escape rules are honoured by the downstream JSON parser.
func loadSpec(t *testing.T, path string) []specEntry {
	t.Helper()
	f, err := os.Open(path)
	if err != nil {
		t.Fatalf("open %s: %v", path, err)
	}
	defer f.Close()

	var out []specEntry
	scanner := bufio.NewScanner(f)
	scanner.Buffer(make([]byte, 1<<20), 1<<20)
	lineNo := 0
	for scanner.Scan() {
		lineNo++
		line := scanner.Text()
		if line == "" || strings.HasPrefix(line, "#") {
			continue
		}
		cols := strings.Split(line, "\t")
		if len(cols) < 3 {
			t.Fatalf("%s:%d: expected at least 3 tab-separated columns, got %d", path, lineNo, len(cols))
		}
		entry := specEntry{
			File:     filepath.Base(path),
			Line:     lineNo,
			Name:     cols[0],
			Input:    unescapeInput(cols[1]),
			Expected: cols[2],
		}
		if len(cols) >= 4 {
			entry.Opts = cols[3]
		}
		out = append(out, entry)
	}
	if err := scanner.Err(); err != nil {
		t.Fatalf("read %s: %v", path, err)
	}
	return out
}

// unescapeInput decodes the escape sequences used in the `input`
// column of the TSV spec: \n (LF), \r (CR), \t (TAB), \\ (backslash).
// Any other `\x` sequence is left intact so XML escapes like `\d` are
// not accidentally rewritten.
func unescapeInput(s string) string {
	if !strings.Contains(s, `\`) {
		return s
	}
	var b strings.Builder
	b.Grow(len(s))
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c == '\\' && i+1 < len(s) {
			switch s[i+1] {
			case 'n':
				b.WriteByte('\n')
				i++
				continue
			case 'r':
				b.WriteByte('\r')
				i++
				continue
			case 't':
				b.WriteByte('\t')
				i++
				continue
			case '\\':
				b.WriteByte('\\')
				i++
				continue
			}
		}
		b.WriteByte(c)
	}
	return b.String()
}

// parseOpts decodes the optional options JSON into a map suitable for
// jsonic.UseDefaults. Empty strings produce an empty map.
func parseOpts(t *testing.T, entry specEntry) map[string]any {
	t.Helper()
	if strings.TrimSpace(entry.Opts) == "" {
		return map[string]any{}
	}
	var out map[string]any
	if err := json.Unmarshal([]byte(entry.Opts), &out); err != nil {
		t.Fatalf("%s:%d: parse opts %q: %v", entry.File, entry.Line, entry.Opts, err)
	}
	return out
}

// parseExpected decodes the expected cell: either a JSON document or
// an `ERROR` / `ERROR:code` marker.
func parseExpected(t *testing.T, entry specEntry) (wantErr bool, errCode string, wantJSON any) {
	t.Helper()
	raw := entry.Expected
	if strings.HasPrefix(raw, "ERROR") {
		rest := strings.TrimPrefix(raw, "ERROR")
		rest = strings.TrimPrefix(rest, ":")
		return true, rest, nil
	}
	if err := json.Unmarshal([]byte(raw), &wantJSON); err != nil {
		t.Fatalf("%s:%d: parse expected JSON %q: %v", entry.File, entry.Line, raw, err)
	}
	return false, "", wantJSON
}

// runSpecFile is the workhorse: it loads one spec file and runs each
// row as its own sub-test.
func runSpecFile(t *testing.T, path string) {
	entries := loadSpec(t, path)
	if len(entries) == 0 {
		t.Fatalf("%s: no spec entries loaded", path)
	}
	for _, entry := range entries {
		entry := entry
		t.Run(entry.Name, func(t *testing.T) {
			opts := parseOpts(t, entry)
			wantErr, errCode, wantVal := parseExpected(t, entry)

			j := jsonic.Make()
			if err := j.UseDefaults(Xml, Defaults, opts); err != nil {
				t.Fatalf("plugin init: %v", err)
			}
			got, err := j.Parse(entry.Input)

			if wantErr {
				if err == nil {
					t.Fatalf("expected parse error, got result %v", got)
				}
				if errCode != "" && !strings.Contains(err.Error(), errCode) {
					t.Fatalf("expected error code %q, got %q", errCode, err.Error())
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected parse error: %v", err)
			}

			// Round-trip the got value through JSON for type normalisation
			// so `[]any` vs concrete slice types compare cleanly against
			// values decoded from the spec via json.Unmarshal.
			gotJSON, err := json.Marshal(got)
			if err != nil {
				t.Fatalf("marshal got: %v", err)
			}
			var gotVal any
			if err := json.Unmarshal(gotJSON, &gotVal); err != nil {
				t.Fatalf("unmarshal got: %v", err)
			}
			if !reflect.DeepEqual(gotVal, wantVal) {
				wantPretty, _ := json.Marshal(wantVal)
				t.Fatalf("\nwant: %s\ngot : %s", wantPretty, gotJSON)
			}
		})
	}
}

func TestBasicSpec(t *testing.T)      { runSpecFile(t, filepath.Join(specDir(), "basic.tsv")) }
func TestAttributesSpec(t *testing.T) { runSpecFile(t, filepath.Join(specDir(), "attributes.tsv")) }
func TestEntitiesSpec(t *testing.T)   { runSpecFile(t, filepath.Join(specDir(), "entities.tsv")) }
func TestNamespacesSpec(t *testing.T) { runSpecFile(t, filepath.Join(specDir(), "namespaces.tsv")) }
func TestStructureSpec(t *testing.T)  { runSpecFile(t, filepath.Join(specDir(), "structure.tsv")) }
func TestErrorsSpec(t *testing.T)     { runSpecFile(t, filepath.Join(specDir(), "errors.tsv")) }
func TestW3CSpec(t *testing.T)        { runSpecFile(t, filepath.Join(specDir(), "w3c.tsv")) }

// --- XML literals embedded in Jsonic source --------------------------------
//
// With `embed: true` the plugin extends Jsonic's own grammar so a literal
// XML element can appear anywhere a Jsonic value is expected. These tests
// exercise that integration: plain Jsonic documents still parse, a lone
// XML literal parses as an element, XML literals inside maps and lists
// parse in place, text with JSON-syntax characters (commas, colons) is
// preserved, and namespaces resolve across the embedded tree.

func embedParser(t *testing.T) *jsonic.Jsonic {
	t.Helper()
	j := jsonic.Make()
	if err := j.UseDefaults(Xml, Defaults, map[string]any{"embed": true}); err != nil {
		t.Fatalf("UseDefaults: %v", err)
	}
	return j
}

func TestEmbedPlainJsonicStillWorks(t *testing.T) {
	j := embedParser(t)
	got, err := j.Parse(`{a:1, b:"two"}`)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	m, ok := got.(map[string]any)
	if !ok {
		t.Fatalf("expected map, got %T", got)
	}
	if m["a"] != float64(1) || m["b"] != "two" {
		t.Fatalf("plain jsonic: got %v", m)
	}
}

func TestEmbedXmlAsTopLevelValue(t *testing.T) {
	j := embedParser(t)
	got, err := j.Parse(`<a>hello</a>`)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	el, ok := got.(map[string]any)
	if !ok {
		t.Fatalf("expected map, got %T", got)
	}
	if el["name"] != "a" {
		t.Fatalf("name: got %v", el["name"])
	}
	children, _ := el["children"].([]any)
	if len(children) != 1 || children[0] != "hello" {
		t.Fatalf("children: got %v", children)
	}
}

func TestEmbedXmlInsideJsonicMap(t *testing.T) {
	j := embedParser(t)
	src := `{
  title: "order-42",
  payload: <order id="42">
    <item qty="2">Widget</item>
    <item qty="1">Gadget</item>
  </order>,
}`
	got, err := j.Parse(src)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	m := got.(map[string]any)
	if m["title"] != "order-42" {
		t.Fatalf("title: got %v", m["title"])
	}
	payload := m["payload"].(map[string]any)
	if payload["name"] != "order" {
		t.Fatalf("payload.name: got %v", payload["name"])
	}
	if a, _ := payload["attributes"].(map[string]any); a["id"] != "42" {
		t.Fatalf("payload.attributes.id: got %v", a["id"])
	}
	children, _ := payload["children"].([]any)
	var items []map[string]any
	for _, c := range children {
		if cm, ok := c.(map[string]any); ok && cm["name"] == "item" {
			items = append(items, cm)
		}
	}
	if len(items) != 2 {
		t.Fatalf("expected 2 item elements, got %d", len(items))
	}
	if a, _ := items[0]["attributes"].(map[string]any); a["qty"] != "2" {
		t.Fatalf("item[0].qty: got %v", a["qty"])
	}
	if ch, _ := items[0]["children"].([]any); len(ch) != 1 || ch[0] != "Widget" {
		t.Fatalf("item[0].children: got %v", ch)
	}
	if a, _ := items[1]["attributes"].(map[string]any); a["qty"] != "1" {
		t.Fatalf("item[1].qty: got %v", a["qty"])
	}
}

func TestEmbedXmlTextPreservesJsonSyntaxChars(t *testing.T) {
	j := embedParser(t)
	got, err := j.Parse(`<a>Hello, World!</a>`)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	children, _ := got.(map[string]any)["children"].([]any)
	if len(children) != 1 || children[0] != "Hello, World!" {
		t.Fatalf("children: got %v", children)
	}

	got2, err := j.Parse(`<a>key: value</a>`)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	children2, _ := got2.(map[string]any)["children"].([]any)
	if len(children2) != 1 || children2[0] != "key: value" {
		t.Fatalf("children2: got %v", children2)
	}
}

func TestEmbedMultipleXmlInsideJsonicList(t *testing.T) {
	j := embedParser(t)
	got, err := j.Parse(`[<a/>, <b>x</b>, <c x="1"/>]`)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	arr, ok := got.([]any)
	if !ok || len(arr) != 3 {
		t.Fatalf("expected 3-element list, got %v", got)
	}
	if arr[0].(map[string]any)["name"] != "a" {
		t.Fatalf("arr[0]: %v", arr[0])
	}
	if ch, _ := arr[1].(map[string]any)["children"].([]any); len(ch) != 1 || ch[0] != "x" {
		t.Fatalf("arr[1].children: %v", ch)
	}
	if a, _ := arr[2].(map[string]any)["attributes"].(map[string]any); a["x"] != "1" {
		t.Fatalf("arr[2].attributes.x: %v", a)
	}
}

func TestEmbedXmlNamespacesResolve(t *testing.T) {
	j := embedParser(t)
	got, err := j.Parse(`{doc: <root xmlns="http://e.example"><child/></root>}`)
	if err != nil {
		t.Fatalf("parse: %v", err)
	}
	doc := got.(map[string]any)["doc"].(map[string]any)
	if doc["namespace"] != "http://e.example" {
		t.Fatalf("doc.namespace: %v", doc["namespace"])
	}
	children, _ := doc["children"].([]any)
	if len(children) != 1 {
		t.Fatalf("expected 1 child, got %d", len(children))
	}
	child := children[0].(map[string]any)
	if child["namespace"] != "http://e.example" {
		t.Fatalf("child.namespace: %v", child["namespace"])
	}
}

// TestSpecDirExists is a sanity check that the shared test/spec folder is
// reachable from the Go test working directory.
func TestSpecDirExists(t *testing.T) {
	info, err := os.Stat(specDir())
	if err != nil {
		t.Fatalf("spec dir: %v", err)
	}
	if !info.IsDir() {
		t.Fatalf("%s is not a directory", specDir())
	}
}

// Compile-time assertion that specEntry stringifies meaningfully in
// error messages (keeps `fmt` import stable if trimmed elsewhere).
var _ = fmt.Sprintf
