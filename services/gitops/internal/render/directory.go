package render

import (
	"bytes"
	"encoding/json"
	"io/fs"
	"os"
	"path/filepath"
	"strings"

	"k8s.io/apimachinery/pkg/apis/meta/v1/unstructured"

	"github.com/argoproj/argo-cd/gitops-engine/pkg/utils/kube"
)

// renderDirectory reads every .yaml, .yml and .json file under dir
// (subdirectories only with recurse), split into documents, the way Argo
// CD's repo-server reads a plain directory: a YAML file that does not parse
// and does not look like a manifest is some other YAML and is skipped; one
// that looks like a manifest is an error.
func renderDirectory(root, dir string, recurse bool, limits Limits) ([]*unstructured.Unstructured, error) {
	var objs []*unstructured.Unstructured
	err := filepath.WalkDir(dir, func(p string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			if p != dir && !recurse {
				return filepath.SkipDir
			}
			return nil
		}
		if !d.Type().IsRegular() {
			return nil
		}
		ext := strings.ToLower(filepath.Ext(d.Name()))
		if ext != ".yaml" && ext != ".yml" && ext != ".json" {
			return nil
		}
		rel, _ := filepath.Rel(root, p)
		rel = filepath.ToSlash(rel)
		info, err := d.Info()
		if err != nil {
			return err
		}
		if info.Size() > limits.MaxFileBytes {
			return errorf("%s is larger than %d MB, the limit for one file", rel, limits.MaxFileBytes>>20)
		}
		data, err := os.ReadFile(p)
		if err != nil {
			return errorf("%s could not be read", rel)
		}
		found, err := documents(rel, ext, data)
		if err != nil {
			return err
		}
		objs = append(objs, found...)
		if len(objs) > limits.MaxResources {
			return errorf("the source renders more than %d resources, the limit", limits.MaxResources)
		}
		return nil
	})
	if err != nil {
		if _, ok := AsError(err); ok {
			return nil, err
		}
		return nil, errorf("the directory could not be read: %v", err)
	}
	return objs, nil
}

func documents(rel, ext string, data []byte) ([]*unstructured.Unstructured, error) {
	looksLikeManifest := func() bool {
		if ext == ".json" {
			return bytes.Contains(data, []byte(`"apiVersion"`)) && bytes.Contains(data, []byte(`"kind"`))
		}
		return bytes.Contains(data, []byte("apiVersion:")) && bytes.Contains(data, []byte("kind:")) &&
			bytes.Contains(data, []byte("metadata:"))
	}
	if ext == ".json" {
		var obj unstructured.Unstructured
		decoder := json.NewDecoder(bytes.NewReader(data))
		if err := decoder.Decode(&obj); err != nil {
			if !looksLikeManifest() {
				return nil, nil
			}
			return nil, errorf("%s is not a valid manifest: %s", rel, shortError(err))
		}
		if decoder.More() {
			return nil, errorf("%s holds more than one JSON object; a JSON file must hold exactly one", rel)
		}
		return []*unstructured.Unstructured{&obj}, nil
	}
	objs, err := kube.SplitYAML(data)
	if err != nil {
		if len(objs) == 0 && !looksLikeManifest() {
			return nil, nil
		}
		return nil, errorf("%s is not valid YAML manifests: %s", rel, shortError(err))
	}
	return objs, nil
}

// shortError keeps a parser's message to one readable line.
func shortError(err error) string {
	msg := err.Error()
	if i := strings.IndexByte(msg, '\n'); i >= 0 {
		msg = msg[:i]
	}
	if len(msg) > 300 {
		msg = msg[:300] + "..."
	}
	return msg
}
