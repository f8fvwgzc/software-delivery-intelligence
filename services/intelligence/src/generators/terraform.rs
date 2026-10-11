//! `infra/<cloud>/{main,variables,outputs}.tf`: minimal but complete stacks
//! for Azure Container Apps, AWS ECS Fargate and Google Cloud Run, each with a
//! registry, networking, the compute service and a managed database when
//! the scan detected one. Plus `backend.tf` for remote state.

use super::{GenInput, artifact};
use crate::domain::model::ArtifactData;

pub fn generate(input: &GenInput) -> Vec<ArtifactData> {
    let cloud = if input.cloud.is_empty() {
        "azure"
    } else {
        input.cloud.as_str()
    };
    let (main, variables, outputs) = match cloud {
        "aws" => (aws_main(input), aws_variables(input), aws_outputs(input)),
        "gcp" => (gcp_main(input), gcp_variables(input), gcp_outputs(input)),
        _ => (
            azure_main(input),
            azure_variables(input),
            azure_outputs(input),
        ),
    };
    let dir = format!("infra/{cloud}");
    let db = input
        .database_engine()
        .map(|e| format!(" and a managed {e} database"))
        .unwrap_or_default();
    vec![
        artifact(
            input,
            &format!("{dir}/main.tf"),
            "hcl",
            format!(
                "Provisions the {cloud} registry, network, compute service{db} for {}.",
                input.repo_name
            ),
            main,
        ),
        artifact(
            input,
            &format!("{dir}/variables.tf"),
            "hcl",
            "Inputs with safe defaults: name, region, image tag and sizing.",
            variables,
        ),
        artifact(
            input,
            &format!("{dir}/outputs.tf"),
            "hcl",
            "Service URL, registry and database endpoints the deploy workflow reads.",
            outputs,
        ),
    ]
}

/// `backend.tf` + a bootstrap of the state storage for the project's cloud.
pub fn remote_state(input: &GenInput) -> Vec<ArtifactData> {
    let cloud = if input.cloud.is_empty() {
        "azure"
    } else {
        input.cloud.as_str()
    };
    let name = input.image.replace('-', "");
    let content = match cloud {
        "aws" => format!(
            "terraform {{
  required_version = \">= 1.6\"

  backend \"s3\" {{
    bucket         = \"{name}-tfstate\"
    key            = \"{image}/terraform.tfstate\"
    region         = \"{region}\"
    dynamodb_table = \"{name}-tfstate-lock\"
    encrypt        = true
  }}
}}
",
            image = input.image,
            region = input.region
        ),
        "gcp" => format!(
            "terraform {{
  required_version = \">= 1.6\"

  backend \"gcs\" {{
    bucket = \"{name}-tfstate\"
    prefix = \"{image}\"
  }}
}}
",
            image = input.image
        ),
        _ => format!(
            "terraform {{
  required_version = \">= 1.6\"

  backend \"azurerm\" {{
    resource_group_name  = \"rg-{image}-tfstate\"
    storage_account_name = \"{name}tfstate\"
    container_name       = \"tfstate\"
    key                  = \"{image}.terraform.tfstate\"
    use_oidc             = true
  }}
}}
",
            image = input.image
        ),
    };
    vec![artifact(
        input,
        &format!("infra/{cloud}/backend.tf"),
        "hcl",
        "Remote, locked Terraform state so plans on pull requests and applies on main share one truth.",
        content,
    )]
}

fn sanitized(input: &GenInput) -> String {
    input.image.replace('-', "")
}

// ---------------------------------------------------------------------------
// Azure
// ---------------------------------------------------------------------------

fn azure_main(input: &GenInput) -> String {
    let name = &input.image;
    let acr = sanitized(input);
    let mut out = format!(
        "terraform {{
  required_version = \">= 1.6\"
  required_providers {{
    azurerm = {{
      source  = \"hashicorp/azurerm\"
      version = \"~> 4.0\"
    }}
  }}
}}

provider \"azurerm\" {{
  features {{}}
  use_oidc = true
}}

resource \"azurerm_resource_group\" \"main\" {{
  name     = \"rg-${{var.name}}\"
  location = var.location
  tags     = var.tags
}}

resource \"azurerm_log_analytics_workspace\" \"main\" {{
  name                = \"log-${{var.name}}\"
  location            = azurerm_resource_group.main.location
  resource_group_name = azurerm_resource_group.main.name
  sku                 = \"PerGB2018\"
  retention_in_days   = 30
  tags                = var.tags
}}

resource \"azurerm_container_registry\" \"main\" {{
  name                = \"acr{acr}\"
  resource_group_name = azurerm_resource_group.main.name
  location            = azurerm_resource_group.main.location
  sku                 = \"Basic\"
  admin_enabled       = false
  tags                = var.tags
}}

resource \"azurerm_user_assigned_identity\" \"app\" {{
  name                = \"id-${{var.name}}\"
  location            = azurerm_resource_group.main.location
  resource_group_name = azurerm_resource_group.main.name
}}

resource \"azurerm_role_assignment\" \"acr_pull\" {{
  scope                = azurerm_container_registry.main.id
  role_definition_name = \"AcrPull\"
  principal_id         = azurerm_user_assigned_identity.app.principal_id
}}

resource \"azurerm_container_app_environment\" \"main\" {{
  name                       = \"cae-${{var.name}}\"
  location                   = azurerm_resource_group.main.location
  resource_group_name        = azurerm_resource_group.main.name
  log_analytics_workspace_id = azurerm_log_analytics_workspace.main.id
  tags                       = var.tags
}}

resource \"azurerm_container_app\" \"app\" {{
  name                         = var.name
  container_app_environment_id = azurerm_container_app_environment.main.id
  resource_group_name          = azurerm_resource_group.main.name
  revision_mode                = \"Single\"
  tags                         = var.tags

  identity {{
    type         = \"UserAssigned\"
    identity_ids = [azurerm_user_assigned_identity.app.id]
  }}

  registry {{
    server   = azurerm_container_registry.main.login_server
    identity = azurerm_user_assigned_identity.app.id
  }}

  ingress {{
    external_enabled = true
    target_port      = var.port
    transport        = \"auto\"
    traffic_weight {{
      latest_revision = true
      percentage      = 100
    }}
  }}

  template {{
    min_replicas = var.min_replicas
    max_replicas = var.max_replicas

    container {{
      name   = var.name
      image  = \"${{azurerm_container_registry.main.login_server}}/${{var.name}}:${{var.image_tag}}\"
      cpu    = var.cpu
      memory = var.memory

      env {{
        name  = \"PORT\"
        value = tostring(var.port)
      }}
{db_env}
      liveness_probe {{
        transport = \"HTTP\"
        path      = \"{health}\"
        port      = var.port
      }}

      readiness_probe {{
        transport = \"HTTP\"
        path      = \"{health}\"
        port      = var.port
      }}
    }}
  }}
}}
",
        health = input.health_path,
        db_env = if input.database_engine().is_some() {
            "
      env {
        name        = \"DATABASE_URL\"
        secret_name = \"database-url\"
      }"
        } else {
            ""
        },
    );
    if input.database_engine().is_some() {
        // The secret block has to live on the app; it references the database
        // declared below, so Terraform orders the creation correctly.
        out = out.replace(
            "  template {\n    min_replicas",
            &format!(
                "  secret {{
    name  = \"database-url\"
    value = \"postgres://${{var.db_admin}}:${{random_password.db.result}}@${{azurerm_postgresql_flexible_server.main.fqdn}}:5432/{db}?sslmode=require\"
  }}

  template {{
    min_replicas",
                db = name.replace('-', "_")
            ),
        );
        out.push_str(&format!(
            "
resource \"random_password\" \"db\" {{
  length  = 32
  special = false
}}

resource \"azurerm_postgresql_flexible_server\" \"main\" {{
  name                   = \"psql-${{var.name}}\"
  resource_group_name    = azurerm_resource_group.main.name
  location               = azurerm_resource_group.main.location
  version                = \"16\"
  administrator_login    = var.db_admin
  administrator_password = random_password.db.result
  sku_name               = var.db_sku
  storage_mb             = 32768
  backup_retention_days  = 7
  zone                   = \"1\"
  tags                   = var.tags
}}

resource \"azurerm_postgresql_flexible_server_database\" \"app\" {{
  name      = \"{db}\"
  server_id = azurerm_postgresql_flexible_server.main.id
  charset   = \"UTF8\"
  collation = \"en_US.utf8\"
}}

resource \"azurerm_postgresql_flexible_server_firewall_rule\" \"azure_services\" {{
  name             = \"allow-azure-services\"
  server_id        = azurerm_postgresql_flexible_server.main.id
  start_ip_address = \"0.0.0.0\"
  end_ip_address   = \"0.0.0.0\"
}}
",
            db = name.replace('-', "_")
        ));
    }
    out
}

fn azure_variables(input: &GenInput) -> String {
    let mut out = format!(
        "variable \"name\" {{
  description = \"Workload name used for every resource.\"
  type        = string
  default     = \"{name}\"
}}

variable \"location\" {{
  description = \"Azure region.\"
  type        = string
  default     = \"{region}\"
}}

variable \"image_tag\" {{
  description = \"Image tag to run; the deploy workflow passes the commit SHA.\"
  type        = string
  default     = \"latest\"
}}

variable \"port\" {{
  type    = number
  default = {port}
}}

variable \"cpu\" {{
  type    = number
  default = 0.5
}}

variable \"memory\" {{
  type    = string
  default = \"1Gi\"
}}

variable \"min_replicas\" {{
  type    = number
  default = 0
}}

variable \"max_replicas\" {{
  type    = number
  default = {replicas}
}}

variable \"tags\" {{
  type = map(string)
  default = {{
    workload   = \"{name}\"
    managed_by = \"terraform\"
  }}
}}
",
        name = input.image,
        region = input.region,
        port = input.port,
        replicas = input.replicas.max(2),
    );
    if input.database_engine().is_some() {
        out.push_str(
            "
variable \"db_admin\" {
  type    = string
  default = \"appadmin\"
}

variable \"db_sku\" {
  description = \"Burstable tier is enough for a small service.\"
  type        = string
  default     = \"B_Standard_B1ms\"
}
",
        );
    }
    out
}

fn azure_outputs(input: &GenInput) -> String {
    let mut out = String::from(
        "output \"app_url\" {
  value = \"https://${azurerm_container_app.app.ingress[0].fqdn}\"
}

output \"registry_login_server\" {
  value = azurerm_container_registry.main.login_server
}

output \"resource_group\" {
  value = azurerm_resource_group.main.name
}
",
    );
    if input.database_engine().is_some() {
        out.push_str(
            "
output \"database_fqdn\" {
  value = azurerm_postgresql_flexible_server.main.fqdn
}
",
        );
    }
    out
}

// ---------------------------------------------------------------------------
// AWS
// ---------------------------------------------------------------------------

fn aws_main(input: &GenInput) -> String {
    let mut out = format!(
        "terraform {{
  required_version = \">= 1.6\"
  required_providers {{
    aws = {{
      source  = \"hashicorp/aws\"
      version = \"~> 5.0\"
    }}
  }}
}}

provider \"aws\" {{
  region = var.region
  default_tags {{
    tags = var.tags
  }}
}}

data \"aws_availability_zones\" \"available\" {{
  state = \"available\"
}}

resource \"aws_vpc\" \"main\" {{
  cidr_block           = \"10.0.0.0/16\"
  enable_dns_hostnames = true
  enable_dns_support   = true
  tags                 = {{ Name = \"${{var.name}}-vpc\" }}
}}

resource \"aws_internet_gateway\" \"main\" {{
  vpc_id = aws_vpc.main.id
}}

resource \"aws_subnet\" \"public\" {{
  count                   = 2
  vpc_id                  = aws_vpc.main.id
  cidr_block              = cidrsubnet(aws_vpc.main.cidr_block, 8, count.index)
  availability_zone       = data.aws_availability_zones.available.names[count.index]
  map_public_ip_on_launch = true
  tags                    = {{ Name = \"${{var.name}}-public-${{count.index}}\" }}
}}

resource \"aws_route_table\" \"public\" {{
  vpc_id = aws_vpc.main.id
  route {{
    cidr_block = \"0.0.0.0/0\"
    gateway_id = aws_internet_gateway.main.id
  }}
}}

resource \"aws_route_table_association\" \"public\" {{
  count          = 2
  subnet_id      = aws_subnet.public[count.index].id
  route_table_id = aws_route_table.public.id
}}

resource \"aws_security_group\" \"alb\" {{
  name   = \"${{var.name}}-alb\"
  vpc_id = aws_vpc.main.id
  ingress {{
    from_port   = 80
    to_port     = 80
    protocol    = \"tcp\"
    cidr_blocks = [\"0.0.0.0/0\"]
  }}
  egress {{
    from_port   = 0
    to_port     = 0
    protocol    = \"-1\"
    cidr_blocks = [\"0.0.0.0/0\"]
  }}
}}

resource \"aws_security_group\" \"service\" {{
  name   = \"${{var.name}}-service\"
  vpc_id = aws_vpc.main.id
  ingress {{
    from_port       = var.port
    to_port         = var.port
    protocol        = \"tcp\"
    security_groups = [aws_security_group.alb.id]
  }}
  egress {{
    from_port   = 0
    to_port     = 0
    protocol    = \"-1\"
    cidr_blocks = [\"0.0.0.0/0\"]
  }}
}}

resource \"aws_ecr_repository\" \"app\" {{
  name                 = var.name
  image_tag_mutability = \"IMMUTABLE\"
  image_scanning_configuration {{
    scan_on_push = true
  }}
}}

resource \"aws_cloudwatch_log_group\" \"app\" {{
  name              = \"/ecs/${{var.name}}\"
  retention_in_days = 30
}}

resource \"aws_ecs_cluster\" \"main\" {{
  name = var.name
  setting {{
    name  = \"containerInsights\"
    value = \"enabled\"
  }}
}}

data \"aws_iam_policy_document\" \"task_assume\" {{
  statement {{
    actions = [\"sts:AssumeRole\"]
    principals {{
      type        = \"Service\"
      identifiers = [\"ecs-tasks.amazonaws.com\"]
    }}
  }}
}}

resource \"aws_iam_role\" \"execution\" {{
  name               = \"${{var.name}}-execution\"
  assume_role_policy = data.aws_iam_policy_document.task_assume.json
}}

resource \"aws_iam_role_policy_attachment\" \"execution\" {{
  role       = aws_iam_role.execution.name
  policy_arn = \"arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy\"
}}

resource \"aws_lb\" \"main\" {{
  name               = var.name
  load_balancer_type = \"application\"
  security_groups    = [aws_security_group.alb.id]
  subnets            = aws_subnet.public[*].id
}}

resource \"aws_lb_target_group\" \"app\" {{
  name        = var.name
  port        = var.port
  protocol    = \"HTTP\"
  vpc_id      = aws_vpc.main.id
  target_type = \"ip\"
  health_check {{
    path                = \"{health}\"
    healthy_threshold   = 2
    unhealthy_threshold = 3
    interval            = 30
  }}
}}

resource \"aws_lb_listener\" \"http\" {{
  load_balancer_arn = aws_lb.main.arn
  port              = 80
  protocol          = \"HTTP\"
  default_action {{
    type             = \"forward\"
    target_group_arn = aws_lb_target_group.app.arn
  }}
}}

resource \"aws_ecs_task_definition\" \"app\" {{
  family                   = var.name
  network_mode             = \"awsvpc\"
  requires_compatibilities = [\"FARGATE\"]
  cpu                      = var.cpu
  memory                   = var.memory
  execution_role_arn       = aws_iam_role.execution.arn
  container_definitions = jsonencode([
    {{
      name      = var.name
      image     = \"${{aws_ecr_repository.app.repository_url}}:${{var.image_tag}}\"
      essential = true
      portMappings = [{{ containerPort = var.port, protocol = \"tcp\" }}]
      environment = [
        {{ name = \"PORT\", value = tostring(var.port) }}{db_env}
      ]
      logConfiguration = {{
        logDriver = \"awslogs\"
        options = {{
          awslogs-group         = aws_cloudwatch_log_group.app.name
          awslogs-region        = var.region
          awslogs-stream-prefix = \"app\"
        }}
      }}
    }}
  ])
}}

resource \"aws_ecs_service\" \"app\" {{
  name            = var.name
  cluster         = aws_ecs_cluster.main.id
  task_definition = aws_ecs_task_definition.app.arn
  desired_count   = var.desired_count
  launch_type     = \"FARGATE\"

  network_configuration {{
    subnets          = aws_subnet.public[*].id
    security_groups  = [aws_security_group.service.id]
    assign_public_ip = true
  }}

  load_balancer {{
    target_group_arn = aws_lb_target_group.app.arn
    container_name   = var.name
    container_port   = var.port
  }}

  depends_on = [aws_lb_listener.http]
}}
",
        health = input.health_path,
        db_env = if input.database_engine().is_some() {
            ",\n        { name = \"DATABASE_URL\", value = local.database_url }"
        } else {
            ""
        },
    );
    if let Some(engine) = input.database_engine() {
        let (tf_engine, port, version, scheme) = if engine == "mysql" {
            ("mysql", 3306, "8.0", "mysql")
        } else {
            ("postgres", 5432, "16", "postgres")
        };
        out.push_str(&format!(
            "
resource \"random_password\" \"db\" {{
  length  = 32
  special = false
}}

resource \"aws_security_group\" \"db\" {{
  name   = \"${{var.name}}-db\"
  vpc_id = aws_vpc.main.id
  ingress {{
    from_port       = {port}
    to_port         = {port}
    protocol        = \"tcp\"
    security_groups = [aws_security_group.service.id]
  }}
}}

resource \"aws_db_subnet_group\" \"main\" {{
  name       = var.name
  subnet_ids = aws_subnet.public[*].id
}}

resource \"aws_db_instance\" \"main\" {{
  identifier             = var.name
  engine                 = \"{tf_engine}\"
  engine_version         = \"{version}\"
  instance_class         = var.db_instance_class
  allocated_storage      = 20
  db_name                = \"{db}\"
  username               = var.db_admin
  password               = random_password.db.result
  db_subnet_group_name   = aws_db_subnet_group.main.name
  vpc_security_group_ids = [aws_security_group.db.id]
  skip_final_snapshot    = true
  backup_retention_period = 7
}}

locals {{
  database_url = \"{scheme}://${{var.db_admin}}:${{random_password.db.result}}@${{aws_db_instance.main.address}}:{port}/{db}\"
}}
",
            db = input.image.replace('-', "_")
        ));
    }
    out
}

fn aws_variables(input: &GenInput) -> String {
    let mut out = format!(
        "variable \"name\" {{
  type    = string
  default = \"{name}\"
}}

variable \"region\" {{
  type    = string
  default = \"{region}\"
}}

variable \"image_tag\" {{
  type    = string
  default = \"latest\"
}}

variable \"port\" {{
  type    = number
  default = {port}
}}

variable \"cpu\" {{
  type    = number
  default = 256
}}

variable \"memory\" {{
  type    = number
  default = 512
}}

variable \"desired_count\" {{
  type    = number
  default = {replicas}
}}

variable \"tags\" {{
  type = map(string)
  default = {{
    workload   = \"{name}\"
    managed_by = \"terraform\"
  }}
}}
",
        name = input.image,
        region = input.region,
        port = input.port,
        replicas = input.replicas,
    );
    if input.database_engine().is_some() {
        out.push_str(
            "
variable \"db_admin\" {
  type    = string
  default = \"appadmin\"
}

variable \"db_instance_class\" {
  type    = string
  default = \"db.t4g.micro\"
}
",
        );
    }
    out
}

fn aws_outputs(input: &GenInput) -> String {
    let mut out = String::from(
        "output \"app_url\" {
  value = \"http://${aws_lb.main.dns_name}\"
}

output \"ecr_repository_url\" {
  value = aws_ecr_repository.app.repository_url
}

output \"ecs_cluster\" {
  value = aws_ecs_cluster.main.name
}
",
    );
    if input.database_engine().is_some() {
        out.push_str(
            "
output \"database_endpoint\" {
  value = aws_db_instance.main.address
}
",
        );
    }
    out
}

// ---------------------------------------------------------------------------
// GCP
// ---------------------------------------------------------------------------

fn gcp_main(input: &GenInput) -> String {
    let mut out = format!(
        "terraform {{
  required_version = \">= 1.6\"
  required_providers {{
    google = {{
      source  = \"hashicorp/google\"
      version = \"~> 6.0\"
    }}
  }}
}}

provider \"google\" {{
  project = var.project_id
  region  = var.region
}}

resource \"google_project_service\" \"apis\" {{
  for_each = toset([
    \"run.googleapis.com\",
    \"artifactregistry.googleapis.com\",
    \"iam.googleapis.com\",{sql_api}
  ])
  service            = each.key
  disable_on_destroy = false
}}

resource \"google_artifact_registry_repository\" \"app\" {{
  location      = var.region
  repository_id = var.name
  format        = \"DOCKER\"
  depends_on    = [google_project_service.apis]
}}

resource \"google_service_account\" \"app\" {{
  account_id   = \"${{var.name}}-run\"
  display_name = \"${{var.name}} Cloud Run runtime\"
}}

resource \"google_cloud_run_v2_service\" \"app\" {{
  name     = var.name
  location = var.region
  ingress  = \"INGRESS_TRAFFIC_ALL\"

  template {{
    service_account = google_service_account.app.email
    scaling {{
      min_instance_count = 0
      max_instance_count = var.max_instances
    }}
    containers {{
      image = \"${{var.region}}-docker.pkg.dev/${{var.project_id}}/${{var.name}}/${{var.name}}:${{var.image_tag}}\"
      ports {{
        container_port = var.port
      }}
      resources {{
        limits = {{
          cpu    = var.cpu
          memory = var.memory
        }}
      }}
      env {{
        name  = \"PORT\"
        value = tostring(var.port)
      }}{db_env}
      startup_probe {{
        http_get {{
          path = \"{health}\"
          port = var.port
        }}
        initial_delay_seconds = 5
        period_seconds        = 10
        failure_threshold     = 6
      }}
      liveness_probe {{
        http_get {{
          path = \"{health}\"
          port = var.port
        }}
        period_seconds = 30
      }}
    }}
  }}
  depends_on = [google_project_service.apis]
}}

resource \"google_cloud_run_v2_service_iam_member\" \"public\" {{
  name     = google_cloud_run_v2_service.app.name
  location = google_cloud_run_v2_service.app.location
  role     = \"roles/run.invoker\"
  member   = \"allUsers\"
}}
",
        health = input.health_path,
        sql_api = if input.database_engine().is_some() {
            "\n    \"sqladmin.googleapis.com\","
        } else {
            ""
        },
        db_env = if input.database_engine().is_some() {
            "
      env {
        name = \"DATABASE_URL\"
        value_source {
          secret_key_ref {
            secret  = google_secret_manager_secret.database_url.secret_id
            version = \"latest\"
          }
        }
      }"
        } else {
            ""
        },
    );
    if let Some(engine) = input.database_engine() {
        let (version, port, scheme) = if engine == "mysql" {
            ("MYSQL_8_0", 3306, "mysql")
        } else {
            ("POSTGRES_16", 5432, "postgres")
        };
        out.push_str(&format!(
            "
resource \"random_password\" \"db\" {{
  length  = 32
  special = false
}}

resource \"google_sql_database_instance\" \"main\" {{
  name             = var.name
  database_version = \"{version}\"
  region           = var.region
  settings {{
    tier = var.db_tier
    backup_configuration {{
      enabled = true
    }}
  }}
  deletion_protection = false
  depends_on          = [google_project_service.apis]
}}

resource \"google_sql_database\" \"app\" {{
  name     = \"{db}\"
  instance = google_sql_database_instance.main.name
}}

resource \"google_sql_user\" \"app\" {{
  name     = var.db_admin
  instance = google_sql_database_instance.main.name
  password = random_password.db.result
}}

resource \"google_secret_manager_secret\" \"database_url\" {{
  secret_id = \"${{var.name}}-database-url\"
  replication {{
    auto {{}}
  }}
}}

resource \"google_secret_manager_secret_version\" \"database_url\" {{
  secret      = google_secret_manager_secret.database_url.id
  secret_data = \"{scheme}://${{var.db_admin}}:${{random_password.db.result}}@${{google_sql_database_instance.main.public_ip_address}}:{port}/{db}\"
}}

resource \"google_secret_manager_secret_iam_member\" \"app\" {{
  secret_id = google_secret_manager_secret.database_url.id
  role      = \"roles/secretmanager.secretAccessor\"
  member    = \"serviceAccount:${{google_service_account.app.email}}\"
}}
",
            db = input.image.replace('-', "_")
        ));
    }
    out
}

fn gcp_variables(input: &GenInput) -> String {
    let mut out = format!(
        "variable \"project_id\" {{
  description = \"GCP project to deploy into.\"
  type        = string
}}

variable \"name\" {{
  type    = string
  default = \"{name}\"
}}

variable \"region\" {{
  type    = string
  default = \"{region}\"
}}

variable \"image_tag\" {{
  type    = string
  default = \"latest\"
}}

variable \"port\" {{
  type    = number
  default = {port}
}}

variable \"cpu\" {{
  type    = string
  default = \"1\"
}}

variable \"memory\" {{
  type    = string
  default = \"512Mi\"
}}

variable \"max_instances\" {{
  type    = number
  default = {replicas}
}}
",
        name = input.image,
        region = input.region,
        port = input.port,
        replicas = input.replicas.max(2),
    );
    if input.database_engine().is_some() {
        out.push_str(
            "
variable \"db_admin\" {
  type    = string
  default = \"appadmin\"
}

variable \"db_tier\" {
  type    = string
  default = \"db-f1-micro\"
}
",
        );
    }
    out
}

fn gcp_outputs(input: &GenInput) -> String {
    let mut out = String::from(
        "output \"app_url\" {
  value = google_cloud_run_v2_service.app.uri
}

output \"artifact_registry\" {
  value = \"${var.region}-docker.pkg.dev/${var.project_id}/${google_artifact_registry_repository.app.repository_id}\"
}
",
    );
    if input.database_engine().is_some() {
        out.push_str(
            "
output \"database_connection_name\" {
  value = google_sql_database_instance.main.connection_name
}
",
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::{input::fixtures, snapshot::assert_snapshot};
    use super::*;

    #[test]
    fn azure_with_postgres_snapshot() {
        let files = generate(&fixtures::input(&fixtures::node_context(), "azure"));
        assert_eq!(files.len(), 3);
        assert_eq!(files[0].path, "infra/azure/main.tf");
        assert_snapshot("terraform_azure_main", &files[0]);
        assert!(
            files[0]
                .content
                .contains("azurerm_postgresql_flexible_server")
        );
        assert!(files[0].content.contains("acrviewengine"));
        assert!(files[1].content.contains("default     = \"westeurope\""));
        assert!(files[2].content.contains("database_fqdn"));
    }

    #[test]
    fn aws_and_gcp_are_complete_with_and_without_a_database() {
        let with_db = fixtures::input(&fixtures::node_context(), "aws");
        let aws = generate(&with_db);
        assert_eq!(aws[0].path, "infra/aws/main.tf");
        assert!(aws[0].content.contains("aws_db_instance"));
        assert!(aws[0].content.contains("aws_ecs_service"));
        assert!(aws[1].content.contains("eu-west-1"));
        let without = fixtures::input(&fixtures::rust_context(), "gcp");
        let gcp = generate(&without);
        assert!(gcp[0].content.contains("google_cloud_run_v2_service"));
        assert!(!gcp[0].content.contains("google_sql_database_instance"));
        assert!(!gcp[1].content.contains("db_tier"));
        let state = remote_state(&with_db);
        assert_eq!(state[0].path, "infra/aws/backend.tf");
        assert!(state[0].content.contains("backend \"s3\""));
        for file in aws.iter().chain(&gcp).chain(&state) {
            assert!(!file.content.contains("TODO"), "{}", file.path);
        }
    }
}
