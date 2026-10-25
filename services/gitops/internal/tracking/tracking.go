// Package tracking is Argo CD's annotation tracking with SDI's annotation
// (CONTRACT.md §24.3): every rendered resource carries
//
//	sdi.dev/tracking-id: <application id>:<group>/<kind>:<namespace>/<name>
//
// and a live resource belongs to an application only when that annotation
// names the application and the resource itself. A copied annotation (a
// resource created by hand from another one's manifest) names a different
// resource and so does not count: SDI never adopts, overwrites or prunes
// what it did not create.
package tracking

import (
	"fmt"
	"strings"

	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"
)

// Annotation is the key every managed resource carries.
const Annotation = "sdi.dev/tracking-id"

// ID is the tracking id of obj for application appID. obj's namespace must
// already be final (the destination namespace for namespaced resources that
// named none; empty for cluster-scoped ones).
func ID(appID string, obj *unstructured.Unstructured) string {
	gvk := obj.GroupVersionKind()
	return fmt.Sprintf("%s:%s/%s:%s/%s", appID, gvk.Group, gvk.Kind, obj.GetNamespace(), obj.GetName())
}

// Set writes the annotation on obj.
func Set(appID string, obj *unstructured.Unstructured) {
	annotations := obj.GetAnnotations()
	if annotations == nil {
		annotations = map[string]string{}
	}
	annotations[Annotation] = ID(appID, obj)
	obj.SetAnnotations(annotations)
}

// Value is a parsed tracking id.
type Value struct {
	AppID     string
	Group     string
	Kind      string
	Namespace string
	Name      string
}

// Parse reads a tracking id; false when it is not one.
func Parse(raw string) (Value, bool) {
	parts := strings.SplitN(raw, ":", 3)
	if len(parts) != 3 || parts[0] == "" {
		return Value{}, false
	}
	group, kind, ok := strings.Cut(parts[1], "/")
	if !ok || kind == "" || strings.Contains(kind, "/") {
		return Value{}, false
	}
	// An empty name is a hook with generateName: the engine names it at
	// sync time.
	namespace, name, ok := strings.Cut(parts[2], "/")
	if !ok || strings.Contains(name, "/") {
		return Value{}, false
	}
	return Value{AppID: parts[0], Group: group, Kind: kind, Namespace: namespace, Name: name}, true
}

// AppOf is the application a live resource's annotation names, when the
// annotation also names that very resource; "" otherwise.
func AppOf(obj *unstructured.Unstructured) string {
	raw := obj.GetAnnotations()[Annotation]
	if raw == "" {
		return ""
	}
	v, ok := Parse(raw)
	if !ok {
		return ""
	}
	gvk := obj.GroupVersionKind()
	if v.Group != gvk.Group || v.Kind != gvk.Kind {
		return ""
	}
	if v.Name == "" {
		// A hook rendered with generateName: the engine named it from that
		// prefix, so the prefix is what identifies it.
		prefix := obj.GetGenerateName()
		if prefix == "" || !strings.HasPrefix(obj.GetName(), prefix) || !isHook(obj) {
			return ""
		}
	} else if v.Name != obj.GetName() {
		return ""
	}
	// A cluster-scoped object has no namespace of its own. Its annotation may
	// still carry one when the kind's scope was unknown at render time (a
	// custom resource whose CRD the same sync creates); as in Argo CD's
	// isSelfReferencedObj, the namespace then does not decide.
	if obj.GetNamespace() != "" && v.Namespace != obj.GetNamespace() {
		return ""
	}
	return v.AppID
}

func isHook(obj *unstructured.Unstructured) bool {
	annotations := obj.GetAnnotations()
	_, argo := annotations["argocd.argoproj.io/hook"]
	_, helm := annotations["helm.sh/hook"]
	return argo || helm
}

// Owns is whether the live resource belongs to application appID.
func Owns(appID string, live *unstructured.Unstructured) bool {
	return live != nil && appID != "" && AppOf(live) == appID
}
