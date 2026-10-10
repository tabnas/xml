package tabnasxml

// The order the rules build an element in (xml-grammar.jsonic sets it
// out), and the two things that order must not cost: the value, which is
// what it always was, and rule depth, which a list of siblings must not
// grow. Mirrors ts/test/build-order.test.ts.
//
// The order is what lets a reader of the rule events (tabnas-transduce's
// incremental source) stream a document as the parse proceeds: each
// element in a node of its own, made when its start tag is read, with its
// members added in their order; the "children" member named in U["key"]
// before its list opens; the list in a node of its own, each child
// appended when it is done; and the root element, once it is done, on the
// start rule, set aside while the white space after it is read, so that
// the rule replacing the start rule to read it does not open holding the
// finished document, and put back when the last rule closes.

import (
	"encoding/json"
	"fmt"
	"strings"
	"testing"

	jsonic "github.com/tabnas/jsonic/go"
	tabnas "github.com/tabnas/parser/go"
)

func pureParser(t *testing.T, opts ...map[string]any) *tabnas.Tabnas {
	t.Helper()
	j := tabnas.Make()
	if err := j.UseDefaults(Xml, Defaults, opts...); err != nil {
		t.Fatal(err)
	}
	return j
}

// shape shows a node: a map by its members, in the order the grammar
// declares them (a Go map keeps none), a list by its length.
func shape(v any) string {
	switch n := v.(type) {
	case map[string]any:
		var keys []string
		for _, k := range xmlElementFields {
			if _, ok := n[k.(string)]; ok {
				keys = append(keys, k.(string))
			}
		}
		return "M{" + strings.Join(keys, ",") + "}"
	case []any:
		return fmt.Sprintf("L[%d]", len(n))
	case nil:
		return "nil"
	}
	if tabnas.IsUndefined(v) {
		return "undefined"
	}
	return fmt.Sprint(v)
}

func TestEachElementIsBuiltInANodeOfItsOwnMemberByMemberInDocumentOrder(t *testing.T) {
	j := pureParser(t)
	var got [][6]string
	j.SubRuleDone(func(rule *tabnas.Rule, _ *tabnas.Context, done tabnas.RuleDone) {
		step := ""
		if done.Alt != nil {
			if done.Alt.P != "" {
				step += "p:" + done.Alt.P
			}
			if done.Alt.R != "" {
				step += "r:" + done.Alt.R
			}
		}
		key, _ := rule.U["key"].(string)
		got = append(got, [6]string{
			string(done.State), rule.Name, fmt.Sprint(rule.D), shape(rule.Node), key, step,
		})
	})
	if _, err := j.Parse("<a x=\"1\">t<b/></a>\n"); err != nil {
		t.Fatal(err)
	}
	want := [][6]string{
		{"o", "xml", "0", "nil", "", "p:element"},
		// The element's node, with the members its start tag gives.
		{"o", "element", "1", "M{name,localName,attributes}", "", "p:head"},
		// head reads nothing: its close comes after those members.
		{"o", "head", "2", "M{name,localName,attributes}", "", ""},
		{"c", "head", "2", "M{name,localName,attributes}", "", "r:content"},
		// content names the member it builds before the list opens.
		{"o", "content", "2", "M{name,localName,attributes}", "children", "p:children"},
		// The list, in a node of its own.
		{"o", "children", "3", "L[0]", "", "p:child"},
		// A text child is in the list at once.
		{"o", "child", "4", "L[1]", "", ""},
		{"c", "child", "4", "L[1]", "", "r:child"},
		// The next child replaces the last, at the same depth.
		{"o", "child", "4", "L[1]", "", "p:element"},
		// A self-closing element is complete when its start tag is read.
		{"o", "element", "5", "M{name,localName,attributes,children}", "", ""},
		{"c", "element", "5", "M{name,localName,attributes,children}", "", ""},
		// A finished element child is appended when the child closes.
		{"c", "child", "4", "L[2]", "", ""},
		// The last child hands the list back to the rule whose node it is.
		{"c", "children", "3", "L[2]", "", ""},
		// The finished list becomes "children".
		{"c", "content", "2", "M{name,localName,attributes,children}", "children", ""},
		{"c", "element", "1", "M{name,localName,attributes,children}", "", ""},
		// The white space after the root replaces the start rule, so the
		// root is set aside and the rule replacing it holds nothing; the
		// root is the result at the last close.
		{"c", "xml", "0", "nil", "", "r:xml"},
		{"o", "xml", "0", "nil", "", ""},
		{"c", "xml", "0", "M{name,localName,attributes,children}", "", ""},
	}
	if len(got) != len(want) {
		t.Fatalf("%d passes, want %d:\n%v", len(got), len(want), got)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Errorf("pass %d: got %v, want %v", i, got[i], want[i])
		}
	}
}

func TestRuleDepthOver10000SiblingsIsWhatOneSiblingNeeds(t *testing.T) {
	maxDepth := func(src string) (int, map[string]any) {
		j := pureParser(t)
		max := 0
		j.SubRuleDone(func(rule *tabnas.Rule, _ *tabnas.Context, _ tabnas.RuleDone) {
			if rule.D > max {
				max = rule.D
			}
		})
		out, err := j.Parse(src)
		if err != nil {
			t.Fatal(err)
		}
		return max, out.(map[string]any)
	}
	one, _ := maxDepth("<r><i>v</i></r>")
	many, root := maxDepth("<r>" + strings.Repeat("<i>v</i>t", 10_000) + "</r>")
	if n := len(root["children"].([]any)); n != 20_000 {
		t.Fatalf("%d children, want 20000", n)
	}
	if many != one {
		t.Errorf("10,000 siblings reach depth %d, one sibling %d", many, one)
	}
	// xml 0, element 1, head and content 2, children 3, child 4, the
	// element <i> 5, its head and content 6, its children 7, its text
	// child 8.
	if one != 8 {
		t.Errorf("one sibling reaches depth %d, want 8", one)
	}
	// A level of nesting costs four rules: element, head (then content),
	// children and child.
	if nested, _ := maxDepth("<r><i><i><i>v</i></i></i></r>"); nested != one+8 {
		t.Errorf("two more levels reach depth %d, want %d", nested, one+8)
	}
}

func TestTheValueIsWhatItWas(t *testing.T) {
	cases := []struct {
		opts map[string]any
		src  string
		want string
	}{
		{nil, `<p:a xmlns:p="urn:p" xml:lang="en">hi<p:b/></p:a>`,
			`{"attributes":{"xmlns:p":"urn:p","xml:lang":"en"},"children":["hi",` +
				`{"attributes":{},"children":[],"lang":"en","localName":"b","name":"p:b","namespace":"urn:p","prefix":"p"}],` +
				`"lang":"en","localName":"a","name":"p:a","namespace":"urn:p","prefix":"p"}`},
		{nil, `<a xmlns:p="u"><b xmlns:p="v" xml:space="preserve"><p:c/></b><p:d/></a>`,
			`{"attributes":{"xmlns:p":"u"},"children":[{"attributes":{"xmlns:p":"v","xml:space":"preserve"},` +
				`"children":[{"attributes":{},"children":[],"localName":"c","name":"p:c","namespace":"v","prefix":"p","space":"preserve"}],` +
				`"localName":"b","name":"b","space":"preserve"},` +
				`{"attributes":{},"children":[],"localName":"d","name":"p:d","namespace":"u","prefix":"p"}],"localName":"a","name":"a"}`},
		{nil, "<?xml version=\"1.0\"?>\n<!-- c -->\n<a xmlns=\"A\"/>\n<!-- d -->\n",
			`{"attributes":{"xmlns":"A"},"children":[],"localName":"a","name":"a","namespace":"A"}`},
		{map[string]any{"namespaces": false}, `<p:a xmlns:p="urn:p"><p:b/></p:a>`,
			`{"attributes":{"xmlns:p":"urn:p"},"children":[{"attributes":{},"children":[],"localName":"p:b","name":"p:b"}],` +
				`"localName":"p:a","name":"p:a"}`},
	}
	for _, c := range cases {
		var j *tabnas.Tabnas
		if c.opts == nil {
			j = pureParser(t)
		} else {
			j = pureParser(t, c.opts)
		}
		out, err := j.Parse(c.src)
		if err != nil {
			t.Fatalf("%s: %v", c.src, err)
		}
		got, _ := json.Marshal(out)
		if string(got) != c.want {
			t.Errorf("%s:\n got %s\nwant %s", c.src, got, c.want)
		}
	}
}

func TestANamespaceViolationFailsTheDocumentWhereItAlwaysDid(t *testing.T) {
	j := pureParser(t, map[string]any{"strictNamespaces": true})
	for src, code := range map[string]string{
		// The first violation in document order, raised when the root
		// element is done: a later one, or a later prefix, does not change
		// the code.
		`<a><p:b/><c xmlns:xml="x"/></a>`: "unbound_prefix",
		`<a><c xmlns:xml="x"/><p:b/></a>`: "reserved_namespace",
		// A malformed document fails as malformed first.
		`<a><p:b/></c>`: "xml_mismatched_tag",
	} {
		_, err := j.Parse(src)
		var te *tabnas.TabnasError
		if err == nil || !asTabnasError(err, &te) || te.Code != code {
			t.Errorf("%s: got %v, want %s", src, err, code)
		}
	}
}

func asTabnasError(err error, target **tabnas.TabnasError) bool {
	te, ok := err.(*tabnas.TabnasError)
	if ok {
		*target = te
	}
	return ok
}

func TestEmbedModeResolvesEachXMLLiteralOnItsOwnAndStopsAtItsFirstViolation(t *testing.T) {
	j := jsonic.Make()
	if err := j.UseDefaults(Xml, Defaults, map[string]any{"embed": true, "strictNamespaces": true}); err != nil {
		t.Fatal(err)
	}
	out, err := j.Parse(`{k: <a xmlns:q="u"><p:b/><q:c><q:d/></q:c></a>, j: <q:e xmlns:q="w"/>}`)
	if err != nil {
		t.Fatal(err)
	}
	got, _ := json.Marshal(out)
	// p:b is the first violation: it keeps its prefix and local name, and
	// the elements after it in document order are left unresolved; the
	// second literal starts afresh.
	want := `{"k":{"attributes":{"xmlns:q":"u"},"children":[` +
		`{"attributes":{},"children":[],"localName":"b","name":"p:b","prefix":"p"},` +
		`{"attributes":{},"children":[{"attributes":{},"children":[],"localName":"q:d","name":"q:d"}],"localName":"q:c","name":"q:c"}],` +
		`"localName":"a","name":"a"},` +
		`"j":{"attributes":{"xmlns:q":"w"},"children":[],"localName":"e","name":"q:e","namespace":"w","prefix":"q"}}`
	if string(got) != want {
		t.Errorf("got  %s\nwant %s", got, want)
	}
}
