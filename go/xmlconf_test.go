package tabnasxml

// ---------------------------------------------------------------------------
// W3C XML Conformance Test Suite (xmlts) — Go runner
//
// Corpus: https://www.w3.org/XML/Test/xmlts20130923.tar.gz
//         sha256 9b61db9f5dbffa545f4b8d78422167083a8568c59bd1129f94138f936cf6fc1f
// Fetched by scripts/fetch-xml-suite.sh into test/xmlconf/ (gitignored — the
// corpus is W3C-owned and is never committed to this repository).
//
// This is the Go mirror of ts/test/xmlconf.test.ts and is deliberately
// allowed to be RED.
//
// WHAT IT REPLACED
//   The previous version of this file skipped (t.Skipf) whenever the corpus
//   was absent — and CI never fetches the corpus, so in CI it always skipped
//   while reporting green. When it did run it asserted pass-rate FLOORS
//   (validSaPassFloor = 118 of 120, notWfSaRejectFloor = 30 of 186); a floor
//   of 30/186 is a test that cannot fail. It also looked at only two
//   directories of one collection (xmltest/valid/sa and xmltest/not-wf/sa),
//   ignoring the other ~2200 catalogued documents, and for valid documents
//   asserted only "did not return an error" — never the parsed VALUE.
//
// SCOPE — see the header of ts/test/xmlconf.test.ts; the two runners must
// agree on scope or the TS/Go parity claim is meaningless.
//   valid   -> must be ACCEPTED, and match the catalog's canonical OUTPUT
//   invalid -> must still be ACCEPTED (this parser is non-validating)
//   not-wf  -> must be REJECTED
//   error   -> reporting is at the processor's discretion, so these are not
//              turned into tests at all (rather than a test asserting nothing)
// ---------------------------------------------------------------------------

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strings"
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
)

const confRoot = "../test/xmlconf"

func catalogPath() string { return filepath.Join(confRoot, "xmlconf.xml") }

// catalogTotal pins the corpus census. A silently shrinking corpus is the
// exact failure mode this project keeps getting bitten by.
const catalogTotal = 2586

// TestMain guarantees the corpus is present before any test runs. `go test`
// has no `pretest` hook, so the fetch is performed here. If the corpus
// cannot be obtained the whole package FAILS — it never skips. A
// conformance suite that quietly does not run is worse than no suite,
// because the green tick is a lie.
func TestMain(m *testing.M) {
	if _, err := os.Stat(catalogPath()); err != nil {
		script := filepath.Join("..", "scripts", "fetch-xml-suite.sh")
		fmt.Fprintf(os.Stderr, "W3C XML conformance corpus missing; running %s\n", script)
		cmd := exec.Command("bash", script)
		cmd.Stdout = os.Stderr
		cmd.Stderr = os.Stderr
		if err := cmd.Run(); err != nil {
			fmt.Fprintf(os.Stderr,
				"\nFATAL: the W3C XML Conformance Test Suite is missing and could not be fetched.\n"+
					"  expected catalog: %s\n"+
					"  fetch failed: %v\n"+
					"  fix: run scripts/fetch-xml-suite.sh (needs network access to w3.org)\n"+
					"  these tests do NOT skip: a conformance suite that silently does not\n"+
					"  run produces a green tick that means nothing.\n", catalogPath(), err)
			os.Exit(1)
		}
	}
	if _, err := os.Stat(catalogPath()); err != nil {
		fmt.Fprintf(os.Stderr, "FATAL: corpus still missing after fetch: %s\n", catalogPath())
		os.Exit(1)
	}
	os.Exit(m.Run())
}

// ---------------------------------------------------------------------------
// Catalog reader
//
// xmlconf.xml is itself XML; parsing it with the parser under test would be
// circular. The <TEST> elements are flat and attribute-only, so a scanner is
// enough. Sub-catalogs arrive through internal-subset SYSTEM entities, and
// xml:base on the enclosing <TESTCASES> gives the directory each URI is
// relative to.
// ---------------------------------------------------------------------------

type confTest struct {
	ID             string
	Type           string // valid | invalid | not-wf | error
	Recommendation string
	Entities       string
	Sections       string
	URI            string // absolute-ish path to the document
	Output         string // path to expected canonical output, "" if none
	Description    string
}

var (
	attrRe   = regexp.MustCompile(`([A-Za-z:._-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')`)
	entityRe = regexp.MustCompile(`<!ENTITY\s+([A-Za-z0-9._-]+)\s+SYSTEM\s+"([^"]+)"\s*>`)
	tokenRe  = regexp.MustCompile(`(?s)<TESTCASES\b([^>]*)>|</TESTCASES\s*>|<TEST\b([^>]*?)/?>(.*?)</TEST\s*>|&([A-Za-z0-9._-]+);`)
	wsRe     = regexp.MustCompile(`\s+`)
)

func attrsOf(tag string) map[string]string {
	out := map[string]string{}
	for _, m := range attrRe.FindAllStringSubmatch(tag, -1) {
		if m[2] != "" || !strings.Contains(m[0], "'") {
			out[m[1]] = m[2]
		} else {
			out[m[1]] = m[3]
		}
	}
	return out
}

// The 2013 catalog's final <TESTCASES> declares xml:base="eduni/namespaces/misc/"
// but ships those files in eduni/misc/. Upstream bug; try both. If neither
// exists the census test fails — a missing corpus file is an error, never a
// silent skip.
func resolveInCorpus(base, rel string) string {
	primary := filepath.Join(confRoot, base, rel)
	if _, err := os.Stat(primary); err == nil {
		return primary
	}
	alt := strings.Replace(primary,
		filepath.Join("eduni", "namespaces", "misc"),
		filepath.Join("eduni", "misc"), 1)
	if _, err := os.Stat(alt); err == nil {
		return alt
	}
	return primary
}

func readCatalog(file, base string, into *[]confTest) error {
	raw, err := os.ReadFile(file)
	if err != nil {
		return err
	}
	body := string(raw)
	dir := filepath.Dir(file)

	ents := map[string]string{}
	for _, m := range entityRe.FindAllStringSubmatch(body, -1) {
		ents[m[1]] = m[2]
	}

	stack := []string{base}
	for _, m := range tokenRe.FindAllStringSubmatch(body, -1) {
		tok := m[0]
		top := stack[len(stack)-1]
		switch {
		case strings.HasPrefix(tok, "<TESTCASES"):
			a := attrsOf(m[1])
			if b, ok := a["xml:base"]; ok && b != "" {
				stack = append(stack, filepath.Join(top, b))
			} else {
				stack = append(stack, top)
			}
		case strings.HasPrefix(tok, "</TESTCASES"):
			if len(stack) > 1 {
				stack = stack[:len(stack)-1]
			}
		case strings.HasPrefix(tok, "<TEST"):
			a := attrsOf(m[2])
			rec := a["RECOMMENDATION"]
			if rec == "" {
				rec = "XML1.0"
			}
			ent := a["ENTITIES"]
			if ent == "none" || ent == "" {
				ent = "none"
			}
			out := ""
			if a["OUTPUT"] != "" {
				out = resolveInCorpus(top, a["OUTPUT"])
			}
			desc := strings.TrimSpace(wsRe.ReplaceAllString(m[3], " "))
			if len(desc) > 160 {
				desc = desc[:160]
			}
			*into = append(*into, confTest{
				ID:             a["ID"],
				Type:           a["TYPE"],
				Recommendation: rec,
				Entities:       ent,
				Sections:       a["SECTIONS"],
				URI:            resolveInCorpus(top, a["URI"]),
				Output:         out,
				Description:    desc,
			})
		case strings.HasPrefix(tok, "&"):
			if sys, ok := ents[m[4]]; ok {
				if err := readCatalog(filepath.Join(dir, sys), top, into); err != nil {
					return err
				}
			}
		}
	}
	return nil
}

func loadCatalog(t *testing.T) []confTest {
	t.Helper()
	var all []confTest
	if err := readCatalog(catalogPath(), ".", &all); err != nil {
		t.Fatalf("read catalog %s: %v", catalogPath(), err)
	}
	return all
}

// claimed reports whether a catalog RECOMMENDATION is inside what
// @tabnas/xml claims: XML 1.0 (all errata editions) and Namespaces 1.0.
// XML1.1 / NS1.1 are a different language version.
func claimed(rec string) bool {
	return strings.HasPrefix(rec, "XML1.0") || strings.HasPrefix(rec, "NS1.0")
}

func inScope(all []confTest) []confTest {
	var out []confTest
	for _, t := range all {
		if claimed(t.Recommendation) {
			out = append(out, t)
		}
	}
	return out
}

// ---------------------------------------------------------------------------
// Canonical XML (James Clark's "first canonical form") — the format of the
// suite's OUTPUT files. Serialising the parse result into it is how the
// VALUE is checked; "it did not error" is not an assertion.
// ---------------------------------------------------------------------------

func canonEscape(s string) string {
	var b strings.Builder
	for _, ch := range s {
		switch ch {
		case '&':
			b.WriteString("&amp;")
		case '<':
			b.WriteString("&lt;")
		case '>':
			b.WriteString("&gt;")
		case '"':
			b.WriteString("&quot;")
		case '\t':
			b.WriteString("&#9;")
		case '\n':
			b.WriteString("&#10;")
		case '\r':
			b.WriteString("&#13;")
		default:
			b.WriteRune(ch)
		}
	}
	return b.String()
}

func canonical(node any) string {
	switch v := node.(type) {
	case string:
		return canonEscape(v)
	case nil:
		return ""
	}
	m := asMap(node)
	if m == nil {
		return canonEscape(fmt.Sprint(node))
	}
	name, _ := m["name"].(string)
	var b strings.Builder
	b.WriteString("<" + name)

	attrs := asMap(m["attributes"])
	names := make([]string, 0, len(attrs))
	for k := range attrs {
		names = append(names, k)
	}
	sort.Strings(names)
	for _, k := range names {
		b.WriteString(" " + k + `="` + canonEscape(fmt.Sprint(attrs[k])) + `"`)
	}
	b.WriteString(">")

	children, _ := m["children"].([]any)
	for _, c := range children {
		b.WriteString(canonical(c))
	}
	b.WriteString("</" + name + ">")
	return b.String()
}

// ---------------------------------------------------------------------------
// Runner
// ---------------------------------------------------------------------------

// confParse uses the documented Go setup from the README.
func confParse(ct confTest) (any, error) {
	body, err := os.ReadFile(ct.URI)
	if err != nil {
		return nil, err
	}
	j := jsonic.Make()
	if err := j.UseDefaults(Xml, Defaults); err != nil {
		return nil, err
	}
	// The corpus mixes UTF-8 / UTF-16 / UTF-32; DecodeBOM transcodes.
	return j.Parse(DecodeBOM(string(body)))
}

func confLabel(ct confTest) string {
	return fmt.Sprintf("%s_%s_%s", ct.ID, ct.Recommendation, ct.Entities)
}

func TestXmlConfCensus(t *testing.T) {
	all := loadCatalog(t)
	if len(all) != catalogTotal {
		t.Errorf("catalog census changed: read %d <TEST> entries, expected %d. "+
			"Either the corpus snapshot changed (check scripts/fetch-xml-suite.sh) "+
			"or the catalog reader regressed. A shrinking corpus must never pass quietly.",
			len(all), catalogTotal)
	}
	var missing []string
	for _, ct := range all {
		if _, err := os.Stat(ct.URI); err != nil {
			missing = append(missing, ct.ID+" -> "+ct.URI)
		}
	}
	if len(missing) > 0 {
		t.Errorf("catalogued documents missing from the corpus:\n  %s", strings.Join(missing, "\n  "))
	}

	scoped := inScope(all)
	byType := map[string]int{}
	for _, ct := range scoped {
		byType[ct.Type]++
	}
	t.Logf("catalog %d: in-scope (XML1.0/NS1.0) %d, out-of-scope (XML1.1/NS1.1) %d",
		len(all), len(scoped), len(all)-len(scoped))
	t.Logf("in-scope by TYPE: %v ('error' tests are reported, not asserted)", byType)
}

func TestXmlConfValid(t *testing.T) {
	for _, ct := range inScope(loadCatalog(t)) {
		if ct.Type != "valid" {
			continue
		}
		t.Run(confLabel(ct), func(t *testing.T) {
			got, err := confParse(ct)
			if err != nil {
				t.Fatalf("valid document was rejected: %s\n  %s\n  %v", ct.URI, ct.Description, err)
			}
			if ct.Output == "" {
				return
			}
			raw, rerr := os.ReadFile(ct.Output)
			if rerr != nil {
				t.Fatalf("read expected output %s: %v", ct.Output, rerr)
			}
			want := DecodeBOM(string(raw))
			if c := canonical(got); c != want {
				t.Errorf("canonical output mismatch for %s\n  doc: %s\n  out: %s\n  want: %q\n  got:  %q",
					ct.ID, ct.URI, ct.Output, want, c)
			}
		})
	}
}

func TestXmlConfInvalidButWellFormed(t *testing.T) {
	for _, ct := range inScope(loadCatalog(t)) {
		if ct.Type != "invalid" {
			continue
		}
		t.Run(confLabel(ct), func(t *testing.T) {
			if _, err := confParse(ct); err != nil {
				t.Errorf("well-formed (though DTD-invalid) document was rejected by a "+
					"non-validating parser: %s\n  %s\n  %v", ct.URI, ct.Description, err)
			}
		})
	}
}

func TestXmlConfNotWellFormed(t *testing.T) {
	for _, ct := range inScope(loadCatalog(t)) {
		if ct.Type != "not-wf" {
			continue
		}
		t.Run(confLabel(ct), func(t *testing.T) {
			got, err := confParse(ct)
			if err == nil {
				t.Errorf("not-well-formed document was ACCEPTED: %s\n  %s\n  parsed as: %v",
					ct.URI, ct.Description, got)
			}
		})
	}
}

// TestXmlConfSummary prints the true numbers in one place so a human reading
// CI output does not have to count two thousand subtest lines. The
// per-document tests above carry the assertions.
func TestXmlConfSummary(t *testing.T) {
	var validTotal, validAccepted, validChecked, validCorrect int
	var notwfTotal, notwfRejected int
	var invalidTotal, invalidAccepted int

	for _, ct := range inScope(loadCatalog(t)) {
		if ct.Type == "error" {
			continue
		}
		_, err := confParse(ct)
		switch ct.Type {
		case "valid":
			validTotal++
			if err == nil {
				validAccepted++
				if ct.Output != "" {
					validChecked++
					raw, rerr := os.ReadFile(ct.Output)
					if rerr == nil {
						got, _ := confParse(ct)
						if canonical(got) == DecodeBOM(string(raw)) {
							validCorrect++
						}
					}
				}
			}
		case "not-wf":
			notwfTotal++
			if err != nil {
				notwfRejected++
			}
		case "invalid":
			invalidTotal++
			if err == nil {
				invalidAccepted++
			}
		}
	}

	// A "valid" test passes only if it parsed AND (when the catalog supplies
	// an expected canonical output) matched it.
	validPass := validAccepted - (validChecked - validCorrect)
	pct := func(a, b int) string {
		if b == 0 {
			return "-"
		}
		return fmt.Sprintf("%.1f%%", 100*float64(a)/float64(b))
	}

	t.Logf("=== W3C XML conformance, Go (xmlts 20130923, XML1.0/NS1.0 scope) ===")
	t.Logf("valid   accepted+correct : %d / %d  (%s)", validPass, validTotal, pct(validPass, validTotal))
	t.Logf("          of which parsed: %d / %d", validAccepted, validTotal)
	t.Logf("          value-compared : %d / %d documents with catalog OUTPUT", validCorrect, validChecked)
	t.Logf("not-wf  rejected         : %d / %d  (%s)", notwfRejected, notwfTotal, pct(notwfRejected, notwfTotal))
	t.Logf("invalid accepted (non-validating): %d / %d  (%s)", invalidAccepted, invalidTotal, pct(invalidAccepted, invalidTotal))
}
