package tracking

import (
	"testing"

	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
)

func obj(apiVersion, kind, namespace, name string) *unstructured.Unstructured {
	u := &unstructured.Unstructured{}
	u.SetAPIVersion(apiVersion)
	u.SetKind(kind)
	u.SetNamespace(namespace)
	u.SetName(name)
	return u
}

const app = "0192f4a0-0000-7000-8000-000000000001"

func TestTheIDNamesTheApplicationAndTheResource(t *testing.T) {
	d := obj("apps/v1", "Deployment", "shop", "web")
	if got := ID(app, d); got != app+":apps/Deployment:shop/web" {
		t.Fatalf("ID = %s", got)
	}
	cm := obj("v1", "ConfigMap", "shop", "settings")
	if got := ID(app, cm); got != app+":/ConfigMap:shop/settings" {
		t.Fatalf("core group: %s", got)
	}
	ns := obj("v1", "Namespace", "", "shop")
	if got := ID(app, ns); got != app+":/Namespace:/shop" {
		t.Fatalf("cluster scoped: %s", got)
	}
	v, ok := Parse(ID(app, d))
	if !ok || v != (Value{AppID: app, Group: "apps", Kind: "Deployment", Namespace: "shop", Name: "web"}) {
		t.Fatalf("Parse = %+v, %v", v, ok)
	}
	for _, bad := range []string{"", "x", "a:b:c", app + ":apps:shop/web", app + ":apps/Deployment:shop", ":apps/Deployment:shop/web", app + ":/ConfigMap:a/b/c"} {
		if _, ok := Parse(bad); ok {
			t.Errorf("Parse(%q) accepted", bad)
		}
	}
}

func TestOnlyTheExactResourceBelongsToTheApplication(t *testing.T) {
	d := obj("apps/v1", "Deployment", "shop", "web")
	Set(app, d)
	if !Owns(app, d) {
		t.Fatal("a rendered resource is its application's")
	}
	if Owns("0192f4a0-0000-7000-8000-000000000002", d) {
		t.Fatal("another application does not own it")
	}
	// A manifest copied by hand keeps the annotation but names another
	// resource: not adopted.
	copied := d.DeepCopy()
	copied.SetName("web-copy")
	if Owns(app, copied) {
		t.Fatal("a copied annotation must not make a resource the application's")
	}
	moved := d.DeepCopy()
	moved.SetNamespace("other")
	if Owns(app, moved) {
		t.Fatal("the same name in another namespace is another resource")
	}
	if Owns(app, obj("apps/v1", "Deployment", "shop", "web")) {
		t.Fatal("a resource without the annotation is nobody's")
	}
	if Owns(app, nil) || Owns("", d) {
		t.Fatal("nil and empty never own")
	}
	// A hook rendered with generateName is named by the engine at sync time.
	hook := obj("batch/v1", "Job", "shop", "")
	hook.SetGenerateName("migrate-")
	hook.SetAnnotations(map[string]string{"argocd.argoproj.io/hook": "PreSync"})
	Set(app, hook)
	hook.SetName("migrate-abc1234-presync-1700000000")
	if !Owns(app, hook) {
		t.Fatal("a generated hook is matched by its prefix")
	}
	notHook := hook.DeepCopy()
	notHook.SetAnnotations(map[string]string{Annotation: notHook.GetAnnotations()[Annotation]})
	if Owns(app, notHook) {
		t.Fatal("only hooks are named by the engine")
	}
	other := hook.DeepCopy()
	other.SetName("unrelated")
	if Owns(app, other) {
		t.Fatal("the name must start with the prefix")
	}
	// A cluster-scoped resource whose scope was unknown at render time
	// carries the destination namespace in its annotation.
	crd := obj("example.com/v1", "Widget", "shop", "w")
	Set(app, crd)
	crd.SetNamespace("")
	if !Owns(app, crd) {
		t.Fatal("a cluster-scoped resource is matched without its namespace")
	}
}
