//! `infra/azure/main.bicep`: the Azure Container Apps stack as Bicep for
//! teams that standardise on it instead of Terraform. Same shape as the
//! Terraform module: log analytics, ACR, managed identity, environment, app
//! and a Postgres flexible server when the scan found one.

use super::{GenInput, artifact};
use crate::domain::model::ArtifactData;

pub fn generate(input: &GenInput) -> Vec<ArtifactData> {
    vec![artifact(
        input,
        "infra/azure/main.bicep",
        "bicep",
        format!(
            "Azure Container Apps environment, registry and app for {}{}; deploy with `az deployment group create`.",
            input.repo_name,
            if input.database_engine().is_some() {
                " plus a Postgres flexible server"
            } else {
                ""
            }
        ),
        bicep(input),
    )]
}

fn bicep(input: &GenInput) -> String {
    let acr = input.image.replace('-', "");
    let db = input.image.replace('-', "_");
    let has_db = input.database_engine().is_some();
    let mut out = format!(
        "@description('Workload name used for every resource.')
param name string = '{name}'

@description('Azure region.')
param location string = resourceGroup().location

@description('Image tag to run; the deploy workflow passes the commit SHA.')
param imageTag string = 'latest'

param port int = {port}
param minReplicas int = 0
param maxReplicas int = {replicas}
{db_params}
var tags = {{
  workload: name
  managedBy: 'bicep'
}}

resource logs 'Microsoft.OperationalInsights/workspaces@2023-09-01' = {{
  name: 'log-${{name}}'
  location: location
  tags: tags
  properties: {{
    sku: {{
      name: 'PerGB2018'
    }}
    retentionInDays: 30
  }}
}}

resource registry 'Microsoft.ContainerRegistry/registries@2023-11-01-preview' = {{
  name: 'acr{acr}'
  location: location
  tags: tags
  sku: {{
    name: 'Basic'
  }}
  properties: {{
    adminUserEnabled: false
  }}
}}

resource identity 'Microsoft.ManagedIdentity/userAssignedIdentities@2023-01-31' = {{
  name: 'id-${{name}}'
  location: location
}}

var acrPullRole = subscriptionResourceId('Microsoft.Authorization/roleDefinitions', '7f951dda-4ed3-4680-a7ca-43fe172d538d')

resource acrPull 'Microsoft.Authorization/roleAssignments@2022-04-01' = {{
  name: guid(registry.id, identity.id, acrPullRole)
  scope: registry
  properties: {{
    roleDefinitionId: acrPullRole
    principalId: identity.properties.principalId
    principalType: 'ServicePrincipal'
  }}
}}

resource environment 'Microsoft.App/managedEnvironments@2024-03-01' = {{
  name: 'cae-${{name}}'
  location: location
  tags: tags
  properties: {{
    appLogsConfiguration: {{
      destination: 'log-analytics'
      logAnalyticsConfiguration: {{
        customerId: logs.properties.customerId
        sharedKey: logs.listKeys().primarySharedKey
      }}
    }}
  }}
}}
{db_resources}
resource app 'Microsoft.App/containerApps@2024-03-01' = {{
  name: name
  location: location
  tags: tags
  identity: {{
    type: 'UserAssigned'
    userAssignedIdentities: {{
      '${{identity.id}}': {{}}
    }}
  }}
  properties: {{
    managedEnvironmentId: environment.id
    configuration: {{
      ingress: {{
        external: true
        targetPort: port
        transport: 'auto'
      }}
      registries: [
        {{
          server: registry.properties.loginServer
          identity: identity.id
        }}
      ]{db_secrets}
    }}
    template: {{
      scale: {{
        minReplicas: minReplicas
        maxReplicas: maxReplicas
      }}
      containers: [
        {{
          name: name
          image: '${{registry.properties.loginServer}}/${{name}}:${{imageTag}}'
          resources: {{
            cpu: json('0.5')
            memory: '1Gi'
          }}
          env: [
            {{
              name: 'PORT'
              value: string(port)
            }}{db_env}
          ]
          probes: [
            {{
              type: 'Liveness'
              httpGet: {{
                path: '{health}'
                port: port
              }}
              periodSeconds: 30
            }}
            {{
              type: 'Readiness'
              httpGet: {{
                path: '{health}'
                port: port
              }}
              periodSeconds: 10
            }}
          ]
        }}
      ]
    }}
  }}
  dependsOn: [
    acrPull
  ]
}}

output appUrl string = 'https://${{app.properties.configuration.ingress.fqdn}}'
output registryLoginServer string = registry.properties.loginServer
",
        name = input.image,
        port = input.port,
        replicas = input.replicas.max(2),
        health = input.health_path,
        db_params = if has_db {
            "
param dbAdmin string = 'appadmin'
@secure()
param dbPassword string
param dbSku string = 'Standard_B1ms'
"
        } else {
            ""
        },
        db_resources = if has_db {
            format!(
                "
resource postgres 'Microsoft.DBforPostgreSQL/flexibleServers@2023-12-01-preview' = {{
  name: 'psql-${{name}}'
  location: location
  tags: tags
  sku: {{
    name: dbSku
    tier: 'Burstable'
  }}
  properties: {{
    version: '16'
    administratorLogin: dbAdmin
    administratorLoginPassword: dbPassword
    storage: {{
      storageSizeGB: 32
    }}
    backup: {{
      backupRetentionDays: 7
      geoRedundantBackup: 'Disabled'
    }}
  }}
}}

resource database 'Microsoft.DBforPostgreSQL/flexibleServers/databases@2023-12-01-preview' = {{
  parent: postgres
  name: '{db}'
  properties: {{
    charset: 'UTF8'
    collation: 'en_US.utf8'
  }}
}}

resource allowAzure 'Microsoft.DBforPostgreSQL/flexibleServers/firewallRules@2023-12-01-preview' = {{
  parent: postgres
  name: 'allow-azure-services'
  properties: {{
    startIpAddress: '0.0.0.0'
    endIpAddress: '0.0.0.0'
  }}
}}
"
            )
        } else {
            String::new()
        },
        db_secrets = if has_db {
            format!(
                "
      secrets: [
        {{
          name: 'database-url'
          value: 'postgres://${{dbAdmin}}:${{dbPassword}}@${{postgres.properties.fullyQualifiedDomainName}}:5432/{db}?sslmode=require'
        }}
      ]"
            )
        } else {
            String::new()
        },
        db_env = if has_db {
            "
            {
              name: 'DATABASE_URL'
              secretRef: 'database-url'
            }"
        } else {
            ""
        },
    );
    if has_db {
        out.push_str("output databaseFqdn string = postgres.properties.fullyQualifiedDomainName\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{input::fixtures, snapshot::assert_snapshot};
    use super::*;

    #[test]
    fn bicep_snapshot_with_postgres() {
        let files = generate(&fixtures::input(&fixtures::node_context(), "azure"));
        assert_snapshot("bicep_azure_postgres", &files[0]);
        assert!(
            files[0]
                .content
                .contains("Microsoft.DBforPostgreSQL/flexibleServers")
        );
        assert!(files[0].content.contains("secretRef: 'database-url'"));
        let plain = generate(&fixtures::input(&fixtures::rust_context(), "azure"));
        assert!(!plain[0].content.contains("flexibleServers"));
        assert!(plain[0].content.contains("param port int = 8080"));
    }
}
