package tabnasxml

// ---------------------------------------------------------------------------
// W3C XML Conformance Test Suite (xmlts) — Go runner
//
// Corpus: https://www.w3.org/XML/Test/xmlts20130923.tar.gz
//         sha256 9b61db9f5dbffa545f4b8d78422167083a8568c59bd1129f94138f936cf6fc1f
// Fetched by scripts/fetch-xml-suite.sh into test/xmlconf/ (gitignored — the
// corpus is W3C-owned and is never committed to this repository). `go test`
// has no `pretest` hook, so TestMain below performs the fetch; the corpus is
// therefore present in CI, where nothing used to fetch it and these tests
// consequently never ran at all.
//
// This is the Go mirror of ts/test/xmlconf.test.ts. The two runners must
// agree on scope, or the TS/Go parity claim is meaningless:
//   valid   -> must be ACCEPTED, and match the catalogue's canonical OUTPUT
//   invalid -> must still be ACCEPTED (this parser is non-validating)
//   not-wf  -> must be REJECTED
//   error   -> reporting is at the processor's discretion, so these are not
//              turned into tests at all, rather than into a test that
//              asserts nothing
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

// TestMain guarantees the corpus is present before any test runs. If it
// cannot be obtained the whole package FAILS — it never skips. A conformance
// suite that quietly does not run is worse than no suite, because the green
// tick then means nothing.
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
					"  expected catalogue: %s\n"+
					"  fetch failed: %v\n"+
					"  fix: run scripts/fetch-xml-suite.sh (needs network access to w3.org)\n"+
					"  these tests do NOT skip.\n", catalogPath(), err)
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
// The narrow guard (xmltest/valid/sa and xmltest/not-wf/sa)
//
// 306 of the catalogue's 2586 documents, asserted as pass-count floors.
// Mirrors the block of the same name in ts/test/xml.test.ts. The
// catalogue-wide runner further down is strictly wider, but these floors are
// a tighter guard on the sub-corpus they cover, so they stay.
// ---------------------------------------------------------------------------

const (
	// Minimum `valid/sa/*.xml` documents that must parse without error.
	// The conformance runner pre-decodes BOMs and supports Unicode tag
	// names, and every one of the 120 documents parses, so the floor is
	// the total.
	validSaPassFloor = 120

	// Minimum `not-wf/sa/*.xml` documents that must be rejected (out of
	// 186). The parser catches structural well-formedness errors (bad
	// tags, unmatched close, unterminated constructs, character data
	// outside the root element, the uppercase-X character reference
	// `&#X..;`, and references to external or unparsed entities where
	// §4.1 forbids them) but does not check most character-level WF
	// constraints or DTD-declaration syntax, so the floor is well below
	// the total and serves as a regression guard. Measured: 74.
	notWfSaRejectFloor = 74
)

// xmlconfRoot used to t.Skipf when the corpus was absent, which in CI (where
// nothing fetched it) meant these tests never ran while reporting green.
// TestMain now guarantees the corpus, so absence is a hard failure.
func xmlconfRoot(t *testing.T) string {
	t.Helper()
	root := filepath.Join("..", "test", "xmlconf")
	info, err := os.Stat(filepath.Join(root, "xmltest"))
	if err != nil || !info.IsDir() {
		t.Fatalf("W3C XML Test Suite not found at %s; run scripts/fetch-xml-suite.sh. "+
			"These tests do NOT skip: a conformance suite that silently does not run "+
			"produces a green tick that means nothing.", root)
	}
	return root
}

func xmlconfFiles(t *testing.T, dir string) []string {
	t.Helper()
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatalf("read %s: %v", dir, err)
	}
	var out []string
	for _, e := range entries {
		if e.IsDir() || !strings.HasSuffix(e.Name(), ".xml") {
			continue
		}
		out = append(out, filepath.Join(dir, e.Name()))
	}
	return out
}

func xmlconfParse(src string) (any, error) {
	j := jsonic.Make()
	if err := j.UseDefaults(Xml, Defaults); err != nil {
		return nil, err
	}
	// The conformance suite mixes UTF-8/16/32 encoded files. Detect
	// the byte-order mark and transcode to UTF-8 so the encoding is
	// transparent to the parser.
	return j.Parse(DecodeBOM(src))
}

func TestXmlConfValidStandalone(t *testing.T) {
	root := xmlconfRoot(t)
	files := xmlconfFiles(t, filepath.Join(root, "xmltest", "valid", "sa"))
	if len(files) == 0 {
		t.Fatalf("no files under xmltest/valid/sa")
	}

	pass := 0
	var failures []string
	for _, path := range files {
		body, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("read %s: %v", path, err)
		}
		if _, perr := xmlconfParse(string(body)); perr != nil {
			failures = append(failures, filepath.Base(path)+": "+
				strings.SplitN(perr.Error(), "\n", 2)[0])
			continue
		}
		pass++
	}

	total := len(files)
	t.Logf("valid/sa: %d / %d parsed successfully", pass, total)
	if pass < validSaPassFloor {
		t.Errorf("valid/sa pass count %d dropped below floor %d (total %d). Sample failures:\n  %s",
			pass, validSaPassFloor, total, strings.Join(firstN(failures, 5), "\n  "))
	}
}

func TestXmlConfNotWellFormedStandalone(t *testing.T) {
	root := xmlconfRoot(t)
	files := xmlconfFiles(t, filepath.Join(root, "xmltest", "not-wf", "sa"))
	if len(files) == 0 {
		t.Fatalf("no files under xmltest/not-wf/sa")
	}

	rejected := 0
	var falseAccepts []string
	for _, path := range files {
		body, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("read %s: %v", path, err)
		}
		if _, perr := xmlconfParse(string(body)); perr != nil {
			rejected++
			continue
		}
		falseAccepts = append(falseAccepts, filepath.Base(path))
	}

	total := len(files)
	t.Logf("not-wf/sa: %d / %d rejected as expected", rejected, total)
	if rejected < notWfSaRejectFloor {
		t.Errorf("not-wf/sa reject count %d dropped below floor %d (total %d). Sample false accepts:\n  %s",
			rejected, notWfSaRejectFloor, total, strings.Join(firstN(falseAccepts, 5), "\n  "))
	}
}

func firstN(list []string, n int) []string {
	if len(list) > n {
		return list[:n]
	}
	return list
}

// ---------------------------------------------------------------------------
// Catalogue reader
//
// xmlconf.xml is itself XML; parsing it with the parser under test would be
// circular. The <TEST> elements are flat and attribute-only, so a scanner is
// enough. Sub-catalogues arrive through internal-subset SYSTEM entities, and
// xml:base on the enclosing <TESTCASES> gives the directory each URI is
// relative to.
// ---------------------------------------------------------------------------

type confTest struct {
	ID             string
	Type           string // valid | invalid | not-wf | error
	Recommendation string
	Entities       string
	Sections       string
	URI            string // path to the document
	Output         string // path to the expected canonical output, "" if none
	Description    string
}

var (
	confAttrRe   = regexp.MustCompile(`([A-Za-z:._-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')`)
	confEntityRe = regexp.MustCompile(`<!ENTITY\s+([A-Za-z0-9._-]+)\s+SYSTEM\s+"([^"]+)"\s*>`)
	confTokenRe  = regexp.MustCompile(`(?s)<TESTCASES\b([^>]*)>|</TESTCASES\s*>|<TEST\b([^>]*?)/?>(.*?)</TEST\s*>|&([A-Za-z0-9._-]+);`)
	confWsRe     = regexp.MustCompile(`\s+`)
)

func confAttrsOf(tag string) map[string]string {
	out := map[string]string{}
	for _, m := range confAttrRe.FindAllStringSubmatch(tag, -1) {
		if m[2] != "" || !strings.Contains(m[0], "'") {
			out[m[1]] = m[2]
		} else {
			out[m[1]] = m[3]
		}
	}
	return out
}

// The 2013 catalogue's final <TESTCASES> declares xml:base="eduni/namespaces/misc/"
// but ships those files in eduni/misc/. Upstream inconsistency; try both. If
// neither exists the census test fails — a missing corpus file is an error,
// never a silent skip.
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
	for _, m := range confEntityRe.FindAllStringSubmatch(body, -1) {
		ents[m[1]] = m[2]
	}

	stack := []string{base}
	for _, m := range confTokenRe.FindAllStringSubmatch(body, -1) {
		tok := m[0]
		top := stack[len(stack)-1]
		switch {
		case strings.HasPrefix(tok, "<TESTCASES"):
			a := confAttrsOf(m[1])
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
			a := confAttrsOf(m[2])
			rec := a["RECOMMENDATION"]
			if rec == "" {
				rec = "XML1.0"
			}
			ent := a["ENTITIES"]
			if ent == "" {
				ent = "none"
			}
			out := ""
			if a["OUTPUT"] != "" {
				out = resolveInCorpus(top, a["OUTPUT"])
			}
			desc := strings.TrimSpace(confWsRe.ReplaceAllString(m[3], " "))
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
		t.Fatalf("read catalogue %s: %v", catalogPath(), err)
	}
	return all
}

// claimed reports whether a catalogue RECOMMENDATION is inside what
// @tabnas/xml claims: XML 1.0 (all errata editions) and Namespaces 1.0.
// XML1.1 / NS1.1 are a different language version.
func claimed(rec string) bool {
	return strings.HasPrefix(rec, "XML1.0") || strings.HasPrefix(rec, "NS1.0")
}

func inScope(all []confTest) []confTest {
	var out []confTest
	for _, ct := range all {
		if claimed(ct.Recommendation) {
			out = append(out, ct)
		}
	}
	return out
}

// ---------------------------------------------------------------------------
// Canonical XML (James Clark's "first canonical form") — the format of the
// suite's OUTPUT files. Serialising the parse result into it is how the VALUE
// is checked; "it did not error" is not a statement about the value.
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
// The catalogue-wide sweep
//
// One pass over every in-scope document. The assertions are counts, set to
// what this parser actually achieves (measured 2026-08-09 on main, and
// re-derivable from the summary this file logs). A floor pinned to the
// measured value fails the moment conformance drops by a single document.
// Raise a floor when conformance genuinely improves; never lower one to make
// a regression pass.
// ---------------------------------------------------------------------------

// catalogTotal pins the corpus census. A silently shrinking corpus would drag
// every count below down with it and read as "no worse than before".
const catalogTotal = 2586

const (
	confValidAcceptFloor    = 728
	confValidCanonicalFloor = 232
	confNotWfRejectFloor    = 438
)

// confParse uses the documented Go setup from the README.
func confParse(ct confTest) (any, error) {
	body, err := os.ReadFile(ct.URI)
	if err != nil {
		return nil, err
	}
	// The corpus mixes UTF-8 / UTF-16 / UTF-32; DecodeBOM transcodes.
	return xmlconfParse(string(body))
}

func TestXmlConfCensus(t *testing.T) {
	all := loadCatalog(t)
	if len(all) != catalogTotal {
		t.Errorf("catalogue census changed: read %d <TEST> entries, expected %d. "+
			"Either the corpus snapshot changed (check scripts/fetch-xml-suite.sh) "+
			"or the catalogue reader regressed. A shrinking corpus must never pass quietly.",
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
	t.Logf("catalogue %d: in-scope (XML1.0/NS1.0) %d, out-of-scope (XML1.1/NS1.1) %d",
		len(all), len(scoped), len(all)-len(scoped))
	t.Logf("in-scope by TYPE: %v ('error' tests are reported, not asserted)", byType)
}

type confSweep struct {
	validTotal, validAccepted, validChecked, validCorrect int
	notwfTotal, notwfRejected                             int
	invalidTotal, invalidAccepted                         int
	validRejected, validMismatched                        []string
	notwfAccepted, invalidRejected                        []string
}

func sweepCatalog(t *testing.T) confSweep {
	t.Helper()
	var s confSweep
	for _, ct := range inScope(loadCatalog(t)) {
		if ct.Type == "error" {
			continue
		}
		got, err := confParse(ct)
		label := ct.ID + " (" + ct.Sections + "): " + ct.URI
		switch ct.Type {
		case "valid":
			s.validTotal++
			if err != nil {
				s.validRejected = append(s.validRejected, label)
				continue
			}
			s.validAccepted++
			if ct.Output != "" {
				s.validChecked++
				raw, rerr := os.ReadFile(ct.Output)
				if rerr != nil {
					t.Errorf("read expected output %s: %v", ct.Output, rerr)
					continue
				}
				if canonical(got) == DecodeBOM(string(raw)) {
					s.validCorrect++
				} else {
					s.validMismatched = append(s.validMismatched, label)
				}
			}
		case "not-wf":
			s.notwfTotal++
			if err != nil {
				s.notwfRejected++
			} else {
				s.notwfAccepted = append(s.notwfAccepted, label)
			}
		case "invalid":
			s.invalidTotal++
			if err == nil {
				s.invalidAccepted++
			} else {
				s.invalidRejected = append(s.invalidRejected, label)
			}
		}
	}
	return s
}

func TestXmlConfCatalog(t *testing.T) {
	s := sweepCatalog(t)

	// valid: must be ACCEPTED. `rmt-e2e-50` (eduni/errata-2e/E50.xml) is the
	// single valid document currently rejected, which is why the floor is 728
	// and not 729.
	if s.validAccepted < confValidAcceptFloor {
		t.Errorf("valid accepted %d / %d dropped below the measured floor %d. Rejected:\n  %s",
			s.validAccepted, s.validTotal, confValidAcceptFloor,
			strings.Join(firstN(s.validRejected, 5), "\n  "))
	}

	// valid: and must produce the right VALUE where the catalogue says what
	// that value is. This is the assertion the narrow suite cannot make.
	if s.validCorrect < confValidCanonicalFloor {
		t.Errorf("canonical-output matches %d / %d dropped below the measured floor %d. Mismatches:\n  %s",
			s.validCorrect, s.validChecked, confValidCanonicalFloor,
			strings.Join(firstN(s.validMismatched, 5), "\n  "))
	}

	// invalid: a non-validating parser must accept every one of these, so
	// this is an exact assertion, not a floor.
	if len(s.invalidRejected) > 0 {
		t.Errorf("a non-validating parser must accept well-formed documents that are merely "+
			"DTD-invalid, but %d of %d were rejected:\n  %s",
			len(s.invalidRejected), s.invalidTotal,
			strings.Join(firstN(s.invalidRejected, 5), "\n  "))
	}

	// not-wf: must be REJECTED.
	if s.notwfRejected < confNotWfRejectFloor {
		t.Errorf("not-wf rejected %d / %d dropped below the measured floor %d. Sample false accepts:\n  %s",
			s.notwfRejected, s.notwfTotal, confNotWfRejectFloor,
			strings.Join(firstN(s.notwfAccepted, 5), "\n  "))
	}

	// The dial: the true numbers in one place, so the current state of
	// conformance can be read off a CI log without counting anything by hand.
	validPass := s.validAccepted - (s.validChecked - s.validCorrect)
	pct := func(a, b int) string {
		if b == 0 {
			return "-"
		}
		return fmt.Sprintf("%.1f%%", 100*float64(a)/float64(b))
	}
	t.Logf("=== W3C XML conformance, Go (xmlts 20130923, XML1.0/NS1.0 scope) ===")
	t.Logf("valid   accepted+correct : %d / %d  (%s)", validPass, s.validTotal, pct(validPass, s.validTotal))
	t.Logf("          of which parsed: %d / %d", s.validAccepted, s.validTotal)
	t.Logf("          value-compared : %d / %d documents with catalogue OUTPUT", s.validCorrect, s.validChecked)
	t.Logf("not-wf  rejected         : %d / %d  (%s)", s.notwfRejected, s.notwfTotal, pct(s.notwfRejected, s.notwfTotal))
	t.Logf("invalid accepted (non-validating): %d / %d  (%s)", s.invalidAccepted, s.invalidTotal, pct(s.invalidAccepted, s.invalidTotal))
}
