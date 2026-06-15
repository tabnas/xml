package xml

import (
	"os"
	"path/filepath"
	"strings"
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
)

// Exercise the parser against the W3C XML Conformance Test Suite
// (xmltest, James Clark's set). The suite is not bundled with the
// repository — run `scripts/fetch-xml-suite.sh` to download it into
// `test/xmlconf/`. When the suite is absent these tests are skipped.
//
// Our parser deliberately doesn't implement every XML 1.0 well-
// formedness constraint (we don't validate character legality, resolve
// DTD-declared entities, or check for all forbidden sequences such as
// `--` inside comments), so the goal of these tests is not 100%
// conformance. Instead each test records how many documents parsed as
// expected and fails only if that count regresses below a stable
// floor. The numbers below were measured against the current parser
// and will move upward as conformance improves.

const (
	// Minimum `valid/sa/*.xml` documents that must parse without error
	// (out of 120). The conformance runner pre-decodes BOMs and
	// supports Unicode tag names, so the floor is set close to the
	// total.
	validSaPassFloor = 118

	// Minimum `not-wf/sa/*.xml` documents that must be rejected. The
	// parser catches structural well-formedness errors (bad tags,
	// unmatched close, unterminated constructs) but does not check
	// many character-level WF constraints, so this floor is set well
	// below total (186) and serves as a regression guard.
	notWfSaRejectFloor = 30
)

func xmlconfRoot(t *testing.T) string {
	t.Helper()
	root := filepath.Join("..", "test", "xmlconf")
	info, err := os.Stat(filepath.Join(root, "xmltest"))
	if err != nil || !info.IsDir() {
		t.Skipf("W3C XML Test Suite not found at %s; run scripts/fetch-xml-suite.sh to enable this test", root)
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
		t.Skipf("no files under xmltest/valid/sa")
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
		t.Skipf("no files under xmltest/not-wf/sa")
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
