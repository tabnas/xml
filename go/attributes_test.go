package tabnasxml

import (
	"encoding/json"
	"reflect"
	"testing"

	tabnas "github.com/tabnas/parser/go"
)

// An element's attributes are a *tabnas.OrderedMap: the attributes in the
// order the tag writes them, then the DOCTYPE defaults the tag leaves out,
// in declaration order. That is the order the TypeScript port's object and
// the Rust port's IndexMap keep. These tests pin it on the bare engine,
// which is what the documentation and the C library build on. The shared
// fixtures cannot: they compare after a JSON round trip, where key order
// is not part of the value.

func parseElement(t *testing.T, src string) map[string]any {
	t.Helper()
	j := tabnas.Make()
	if err := j.UseDefaults(Xml, Defaults); err != nil {
		t.Fatal(err)
	}
	out, err := j.Parse(src)
	if err != nil {
		t.Fatalf("%s: %v", src, err)
	}
	el, ok := out.(map[string]any)
	if !ok {
		t.Fatalf("%s: result is %T, not an element map", src, out)
	}
	return el
}

func orderedAttributes(t *testing.T, el map[string]any) *tabnas.OrderedMap {
	t.Helper()
	attrs, ok := el["attributes"].(*tabnas.OrderedMap)
	if !ok || attrs == nil {
		t.Fatalf("attributes is %T, not a *tabnas.OrderedMap", el["attributes"])
	}
	return attrs
}

func TestAttributesKeepSourceOrder(t *testing.T) {
	el := parseElement(t, `<a z="1" b="2" a="3" xmlns:q="urn:q" q:m="4"><b y="5" x="6"/></a>`)

	attrs := orderedAttributes(t, el)
	if want := []string{"z", "b", "a", "xmlns:q", "q:m"}; !reflect.DeepEqual(attrs.Keys, want) {
		t.Errorf("keys = %v, want %v", attrs.Keys, want)
	}
	if v, _ := attrs.Get("a"); v != "3" {
		t.Errorf(`attrs["a"] = %v, want "3"`, v)
	}

	// encoding/json writes them in the same order.
	got, err := json.Marshal(attrs)
	if err != nil {
		t.Fatal(err)
	}
	if want := `{"z":"1","b":"2","a":"3","xmlns:q":"urn:q","q:m":"4"}`; string(got) != want {
		t.Errorf("json = %s, want %s", got, want)
	}

	// A child element's attributes are ordered the same way.
	child, _ := el["children"].([]any)[0].(map[string]any)
	if want := []string{"y", "x"}; !reflect.DeepEqual(orderedAttributes(t, child).Keys, want) {
		t.Errorf("child keys = %v, want %v", orderedAttributes(t, child).Keys, want)
	}

	// An element without attributes still carries an (empty) ordered map.
	if empty := orderedAttributes(t, parseElement(t, `<a/>`)); empty.Len() != 0 {
		t.Errorf("empty element has attributes %v", empty.Keys)
	}
}

func TestAttributeDefaultsFollowInDeclarationOrder(t *testing.T) {
	// Two ATTLIST declarations for one element. z is declared twice: it
	// keeps its first place and takes the later value, as the TypeScript
	// port's Object.assign merge does.
	const doctype = `<!DOCTYPE a [<!ATTLIST a z CDATA "1" b CDATA "2">` +
		`<!ATTLIST a c CDATA "3" z CDATA "9">]>`
	for _, c := range []struct {
		src  string
		keys []string
		json string
	}{
		{doctype + `<a y="0"/>`, []string{"y", "z", "b", "c"}, `{"y":"0","z":"9","b":"2","c":"3"}`},
		{doctype + `<a b="0"/>`, []string{"b", "z", "c"}, `{"b":"0","z":"9","c":"3"}`},
		{doctype + `<a/>`, []string{"z", "b", "c"}, `{"z":"9","b":"2","c":"3"}`},
	} {
		attrs := orderedAttributes(t, parseElement(t, c.src))
		if !reflect.DeepEqual(attrs.Keys, c.keys) {
			t.Errorf("%s: keys = %v, want %v", c.src, attrs.Keys, c.keys)
		}
		got, err := json.Marshal(attrs)
		if err != nil {
			t.Fatal(err)
		}
		if string(got) != c.json {
			t.Errorf("%s: json = %s, want %s", c.src, got, c.json)
		}
	}
}

// When one element breaks two namespace rules, the code reported is the
// one its first offending attribute breaks. Ranging over a plain Go map
// picked one of the two at random; the shared fixtures in
// test/spec/errors.tsv pin the same rows in every runtime, and this
// repeats them to make sure the answer does not vary between parses.
func TestNamespaceErrorFollowsAttributeOrder(t *testing.T) {
	j := tabnas.Make()
	if err := j.UseDefaults(Xml, Defaults); err != nil {
		t.Fatal(err)
	}
	for src, want := range map[string]string{
		`<a xmlns:p="a b" xmlns:xmlns="x"/>`:                       "invalid_namespace_uri",
		`<a xmlns:xmlns="x" xmlns:p="a b"/>`:                       "reserved_namespace",
		`<a xmlns="http://www.w3.org/2000/xmlns/" xmlns:p="a b"/>`: "reserved_namespace",
		`<a xmlns:p="a b" xmlns="http://www.w3.org/2000/xmlns/"/>`: "invalid_namespace_uri",
	} {
		for i := 0; i < 50; i++ {
			_, err := j.Parse(src)
			te, ok := err.(*tabnas.TabnasError)
			if !ok || te.Code != want {
				t.Fatalf("%s (parse %d): got %v, want code %s", src, i+1, err, want)
			}
		}
	}
}
