package mask

import (
	"encoding/json"
	"strings"
	"testing"

	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
)

func secret(data map[string]any, stringData map[string]any, lastApplied bool) *unstructured.Unstructured {
	s := &unstructured.Unstructured{Object: map[string]any{
		"apiVersion": "v1", "kind": "Secret",
		"metadata": map[string]any{"name": "db", "namespace": "shop"},
	}}
	if data != nil {
		s.Object["data"] = data
	}
	if stringData != nil {
		s.Object["stringData"] = stringData
	}
	if lastApplied {
		raw, _ := json.Marshal(s.Object)
		s.SetAnnotations(map[string]string{"kubectl.kubernetes.io/last-applied-configuration": string(raw)})
	}
	return s
}

func dump(t *testing.T, objs ...*unstructured.Unstructured) string {
	t.Helper()
	var b strings.Builder
	for _, o := range objs {
		if o == nil {
			continue
		}
		raw, err := json.Marshal(o.Object)
		if err != nil {
			t.Fatal(err)
		}
		b.Write(raw)
	}
	return b.String()
}

func TestSecretValuesNeverLeave(t *testing.T) {
	// "hunter2" and "s3cret" in base64.
	target := secret(map[string]any{"password": "aHVudGVyMg=="}, map[string]any{"token": "plain-token"}, false)
	live := secret(map[string]any{"password": "czNjcmV0"}, nil, true)
	mt, ml, err := Pair(target, live)
	if err != nil {
		t.Fatal(err)
	}
	text := dump(t, mt, ml)
	for _, leaked := range []string{"aHVudGVyMg==", "czNjcmV0", "plain-token"} {
		if strings.Contains(text, leaked) {
			t.Fatalf("%s leaked: %s", leaked, text)
		}
	}
	if !strings.Contains(text, "++++++++") {
		t.Fatalf("values are not masked: %s", text)
	}
	// Different values get different masks, so the diff still shows a change.
	tp, _, _ := unstructured.NestedString(mt.Object, "data", "password")
	lp, _, _ := unstructured.NestedString(ml.Object, "data", "password")
	if tp == lp {
		t.Fatalf("changed values must mask differently: %q %q", tp, lp)
	}
	// stringData is folded into data and masked too.
	if tok, _, _ := unstructured.NestedString(mt.Object, "data", "token"); !strings.HasPrefix(tok, "++++") {
		t.Fatalf("stringData not masked: %q", tok)
	}
	// The inputs are untouched.
	if v, _, _ := unstructured.NestedString(target.Object, "data", "password"); v != "aHVudGVyMg==" {
		t.Fatal("the input was modified")
	}
}

func TestEqualValuesMaskTheSame(t *testing.T) {
	target := secret(map[string]any{"a": "eA=="}, nil, false)
	live := secret(map[string]any{"a": "eA=="}, nil, false)
	mt, ml, err := Pair(target, live)
	if err != nil {
		t.Fatal(err)
	}
	if dump(t, mt) != dump(t, ml) {
		t.Fatal("equal secrets must mask equal, or every Secret would always look changed")
	}
}

func TestOtherKindsAreLeftAsTheyAre(t *testing.T) {
	cm := &unstructured.Unstructured{Object: map[string]any{
		"apiVersion": "v1", "kind": "ConfigMap",
		"metadata": map[string]any{"name": "c"}, "data": map[string]any{"k": "v"},
	}}
	m, err := One(cm)
	if err != nil || m != cm {
		t.Fatalf("a ConfigMap must pass through: %v", err)
	}
	if out, err := One(secret(map[string]any{"k": "dg=="}, nil, false)); err != nil || strings.Contains(dump(t, out), "dg==") {
		t.Fatalf("One must mask: %v", err)
	}
	r := Redacted(secret(map[string]any{"k": "dg=="}, map[string]any{"x": "y"}, true))
	if text := dump(t, r); strings.Contains(text, "dg==") || strings.Contains(text, `"y"`) {
		t.Fatalf("Redacted leaks: %s", text)
	}
}
