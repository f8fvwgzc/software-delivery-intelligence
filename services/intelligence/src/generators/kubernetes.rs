//! `deploy/k8s/{deployment,service,ingress}.yaml` plus a Helm chart
//! skeleton under `charts/<name>/` whose values carry the same image, port,
//! health path and replica count, so either path deploys the same thing.

use super::{GenInput, artifact};
use crate::domain::model::ArtifactData;

pub fn generate(input: &GenInput) -> Vec<ArtifactData> {
    let chart = format!("charts/{}", input.image);
    vec![
        artifact(
            input,
            "deploy/k8s/deployment.yaml",
            "yaml",
            format!(
                "Deployment of {} with {} replica(s), resource limits, probes on {} and a non-root security context.",
                input.image, input.replicas, input.health_path
            ),
            deployment(input),
        ),
        artifact(
            input,
            "deploy/k8s/service.yaml",
            "yaml",
            "ClusterIP service in front of the pods.",
            service(input),
        ),
        artifact(
            input,
            "deploy/k8s/ingress.yaml",
            "yaml",
            "Ingress routing the public hostname to the service with TLS via cert-manager.",
            ingress(input),
        ),
        artifact(
            input,
            &format!("{chart}/Chart.yaml"),
            "yaml",
            "Helm chart metadata.",
            chart_yaml(input),
        ),
        artifact(
            input,
            &format!("{chart}/values.yaml"),
            "yaml",
            "Chart values: image, replicas, port, health path, ingress host and resources.",
            values(input),
        ),
        artifact(
            input,
            &format!("{chart}/templates/_helpers.tpl"),
            "text",
            "Name and label helpers shared by the templates.",
            helpers(input),
        ),
        artifact(
            input,
            &format!("{chart}/templates/deployment.yaml"),
            "yaml",
            "Templated Deployment driven by values.yaml.",
            chart_deployment(),
        ),
        artifact(
            input,
            &format!("{chart}/templates/service.yaml"),
            "yaml",
            "Templated Service.",
            chart_service(),
        ),
        artifact(
            input,
            &format!("{chart}/templates/ingress.yaml"),
            "yaml",
            "Templated Ingress, enabled through values.",
            chart_ingress(),
        ),
    ]
}

fn host(input: &GenInput) -> String {
    format!("{}.example.com", input.image)
}

fn deployment(input: &GenInput) -> String {
    format!(
        "apiVersion: apps/v1
kind: Deployment
metadata:
  name: {name}
  labels:
    app.kubernetes.io/name: {name}
spec:
  replicas: {replicas}
  selector:
    matchLabels:
      app.kubernetes.io/name: {name}
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxUnavailable: 0
      maxSurge: 1
  template:
    metadata:
      labels:
        app.kubernetes.io/name: {name}
    spec:
      securityContext:
        runAsNonRoot: true
        runAsUser: 10001
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: {name}
          image: {image}:latest
          imagePullPolicy: IfNotPresent
          ports:
            - name: http
              containerPort: {port}
          env:
            - name: PORT
              value: \"{port}\"
          readinessProbe:
            httpGet:
              path: {health}
              port: http
            periodSeconds: 10
          livenessProbe:
            httpGet:
              path: {health}
              port: http
            periodSeconds: 30
          resources:
            requests:
              cpu: 100m
              memory: 128Mi
            limits:
              cpu: 500m
              memory: 512Mi
          securityContext:
            allowPrivilegeEscalation: false
            readOnlyRootFilesystem: false
            capabilities:
              drop: [\"ALL\"]
",
        name = input.image,
        image = input.image_ref(),
        port = input.port,
        replicas = input.replicas,
        health = input.health_path,
    )
}

fn service(input: &GenInput) -> String {
    format!(
        "apiVersion: v1
kind: Service
metadata:
  name: {name}
  labels:
    app.kubernetes.io/name: {name}
spec:
  type: ClusterIP
  selector:
    app.kubernetes.io/name: {name}
  ports:
    - name: http
      port: 80
      targetPort: http
",
        name = input.image
    )
}

fn ingress(input: &GenInput) -> String {
    format!(
        "apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: {name}
  annotations:
    cert-manager.io/cluster-issuer: letsencrypt
spec:
  ingressClassName: nginx
  tls:
    - hosts: [{host}]
      secretName: {name}-tls
  rules:
    - host: {host}
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: {name}
                port:
                  name: http
",
        name = input.image,
        host = host(input),
    )
}

fn chart_yaml(input: &GenInput) -> String {
    format!(
        "apiVersion: v2
name: {name}
description: Helm chart for {repo}
type: application
version: 0.1.0
appVersion: \"latest\"
",
        name = input.image,
        repo = input.repo_name,
    )
}

fn values(input: &GenInput) -> String {
    format!(
        "replicaCount: {replicas}

image:
  repository: {image}
  tag: latest
  pullPolicy: IfNotPresent

service:
  type: ClusterIP
  port: 80
  targetPort: {port}

healthPath: {health}

ingress:
  enabled: true
  className: nginx
  host: {host}
  tls: true
  annotations:
    cert-manager.io/cluster-issuer: letsencrypt

resources:
  requests:
    cpu: 100m
    memory: 128Mi
  limits:
    cpu: 500m
    memory: 512Mi

env:
  PORT: \"{port}\"
",
        replicas = input.replicas,
        image = input.image_ref(),
        port = input.port,
        health = input.health_path,
        host = host(input),
    )
}

fn helpers(input: &GenInput) -> String {
    format!(
        "{{{{- define \"{name}.name\" -}}}}
{{{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix \"-\" -}}}}
{{{{- end -}}}}

{{{{- define \"{name}.labels\" -}}}}
app.kubernetes.io/name: {{{{ include \"{name}.name\" . }}}}
app.kubernetes.io/instance: {{{{ .Release.Name }}}}
app.kubernetes.io/managed-by: {{{{ .Release.Service }}}}
helm.sh/chart: {{{{ .Chart.Name }}}}-{{{{ .Chart.Version }}}}
{{{{- end -}}}}

{{{{- define \"{name}.selectorLabels\" -}}}}
app.kubernetes.io/name: {{{{ include \"{name}.name\" . }}}}
app.kubernetes.io/instance: {{{{ .Release.Name }}}}
{{{{- end -}}}}
",
        name = input.image
    )
}

fn chart_deployment() -> String {
    "apiVersion: apps/v1
kind: Deployment
metadata:
  name: {{ include (printf \"%s.name\" .Chart.Name) . }}
  labels:
    {{- include (printf \"%s.labels\" .Chart.Name) . | nindent 4 }}
spec:
  replicas: {{ .Values.replicaCount }}
  selector:
    matchLabels:
      {{- include (printf \"%s.selectorLabels\" .Chart.Name) . | nindent 6 }}
  template:
    metadata:
      labels:
        {{- include (printf \"%s.selectorLabels\" .Chart.Name) . | nindent 8 }}
    spec:
      securityContext:
        runAsNonRoot: true
        runAsUser: 10001
        seccompProfile:
          type: RuntimeDefault
      containers:
        - name: {{ .Chart.Name }}
          image: \"{{ .Values.image.repository }}:{{ .Values.image.tag }}\"
          imagePullPolicy: {{ .Values.image.pullPolicy }}
          ports:
            - name: http
              containerPort: {{ .Values.service.targetPort }}
          env:
            {{- range $key, $value := .Values.env }}
            - name: {{ $key }}
              value: {{ $value | quote }}
            {{- end }}
          readinessProbe:
            httpGet:
              path: {{ .Values.healthPath }}
              port: http
            periodSeconds: 10
          livenessProbe:
            httpGet:
              path: {{ .Values.healthPath }}
              port: http
            periodSeconds: 30
          resources:
            {{- toYaml .Values.resources | nindent 12 }}
          securityContext:
            allowPrivilegeEscalation: false
            capabilities:
              drop: [\"ALL\"]
"
    .to_owned()
}

fn chart_service() -> String {
    "apiVersion: v1
kind: Service
metadata:
  name: {{ include (printf \"%s.name\" .Chart.Name) . }}
  labels:
    {{- include (printf \"%s.labels\" .Chart.Name) . | nindent 4 }}
spec:
  type: {{ .Values.service.type }}
  selector:
    {{- include (printf \"%s.selectorLabels\" .Chart.Name) . | nindent 4 }}
  ports:
    - name: http
      port: {{ .Values.service.port }}
      targetPort: http
"
    .to_owned()
}

fn chart_ingress() -> String {
    "{{- if .Values.ingress.enabled }}
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: {{ include (printf \"%s.name\" .Chart.Name) . }}
  labels:
    {{- include (printf \"%s.labels\" .Chart.Name) . | nindent 4 }}
  {{- with .Values.ingress.annotations }}
  annotations:
    {{- toYaml . | nindent 4 }}
  {{- end }}
spec:
  ingressClassName: {{ .Values.ingress.className }}
  {{- if .Values.ingress.tls }}
  tls:
    - hosts: [{{ .Values.ingress.host }}]
      secretName: {{ include (printf \"%s.name\" .Chart.Name) . }}-tls
  {{- end }}
  rules:
    - host: {{ .Values.ingress.host }}
      http:
        paths:
          - path: /
            pathType: Prefix
            backend:
              service:
                name: {{ include (printf \"%s.name\" .Chart.Name) . }}
                port:
                  name: http
{{- end }}
"
    .to_owned()
}

#[cfg(test)]
mod tests {
    use super::super::{input::fixtures, snapshot::assert_snapshot};
    use super::*;

    #[test]
    fn manifests_and_chart_snapshot() {
        let files = generate(&fixtures::input(&fixtures::node_context(), ""));
        assert_eq!(files.len(), 9);
        assert_snapshot("k8s_deployment_node", &files[0]);
        assert!(files[0].content.contains("containerPort: 3000"));
        assert!(files[0].content.contains("path: /healthz"));
        assert!(files[0].content.contains("runAsNonRoot: true"));
        assert_eq!(files[3].path, "charts/view-engine/Chart.yaml");
        assert!(
            files[4]
                .content
                .contains("repository: ghcr.io/f8fvwgzc/view-engine")
        );
        assert!(files[5].content.contains("define \"view-engine.name\""));
        for file in &files {
            assert!(!file.content.contains("TODO"), "{}", file.path);
        }
    }
}
