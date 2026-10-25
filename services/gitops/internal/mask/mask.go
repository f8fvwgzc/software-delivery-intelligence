// Package mask hides Secret values wherever a manifest or a diff leaves the
// service (CONTRACT.md §24.4), the way Argo CD does: every value of a
// Secret's data and stringData becomes a run of '+' (the engine's
// diff.HideSecretData, which also covers the kubectl last-applied
// annotation), and a value that differs between desired and live gets a
// different run, so a diff still shows that something changed without
// showing what.
package mask

import (
	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"

	"github.com/argoproj/argo-cd/gitops-engine/pkg/diff"
)

// IsSecret is a core v1 Secret.
func IsSecret(obj *unstructured.Unstructured) bool {
	if obj == nil {
		return false
	}
	gvk := obj.GroupVersionKind()
	return gvk.Group == "" && gvk.Kind == "Secret"
}

// Pair masks a desired and a live object together (either may be nil);
// objects that are not Secrets come back as they were. The inputs are
// never modified.
func Pair(target, live *unstructured.Unstructured) (*unstructured.Unstructured, *unstructured.Unstructured, error) {
	if !IsSecret(target) && !IsSecret(live) {
		return target, live, nil
	}
	return diff.HideSecretData(target, live, nil)
}

// One masks a single object.
func One(obj *unstructured.Unstructured) (*unstructured.Unstructured, error) {
	masked, _, err := Pair(obj, nil)
	return masked, err
}

// Redacted is what a Secret's values read as when masking itself failed:
// the object without any data at all, so a failure never leaks.
func Redacted(obj *unstructured.Unstructured) *unstructured.Unstructured {
	if obj == nil {
		return nil
	}
	out := obj.DeepCopy()
	if IsSecret(out) {
		unstructured.RemoveNestedField(out.Object, "data")
		unstructured.RemoveNestedField(out.Object, "stringData")
		annotations := out.GetAnnotations()
		if _, ok := annotations["kubectl.kubernetes.io/last-applied-configuration"]; ok {
			annotations["kubectl.kubernetes.io/last-applied-configuration"] = "++++++++"
			out.SetAnnotations(annotations)
		}
	}
	return out
}
