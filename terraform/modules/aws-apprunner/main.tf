resource "aws_ecr_repository" "repo" {
  name                 = var.repository_name
  image_tag_mutability = "MUTABLE"
  force_delete         = true

  image_scanning_configuration {
    scan_on_push = true
  }
}

# App Runner pulls private ECR images using this build access role.
data "aws_iam_policy_document" "apprunner_assume" {
  statement {
    actions = ["sts:AssumeRole"]
    principals {
      type        = "Service"
      identifiers = ["build.apprunner.amazonaws.com"]
    }
  }
}

resource "aws_iam_role" "apprunner_ecr" {
  name               = "${var.service_name}-ecr-access"
  assume_role_policy = data.aws_iam_policy_document.apprunner_assume.json
}

resource "aws_iam_role_policy_attachment" "apprunner_ecr" {
  role       = aws_iam_role.apprunner_ecr.name
  policy_arn = "arn:aws:iam::aws:policy/service-role/AWSAppRunnerServicePolicyForECRAccess"
}

resource "aws_apprunner_auto_scaling_configuration_version" "web" {
  auto_scaling_configuration_name = var.service_name

  min_size = var.min_instances
  max_size = var.max_instances

  lifecycle {
    create_before_destroy = true
  }
}

resource "aws_apprunner_service" "web" {
  service_name = var.service_name

  source_configuration {
    auto_deployments_enabled = var.auto_deployments_enabled

    authentication_configuration {
      access_role_arn = aws_iam_role.apprunner_ecr.arn
    }

    image_repository {
      image_identifier      = var.image
      image_repository_type = "ECR"

      image_configuration {
        port                          = tostring(var.container_port)
        runtime_environment_variables = var.env
      }
    }
  }

  instance_configuration {
    cpu    = var.cpu
    memory = var.memory
  }

  auto_scaling_configuration_arn = aws_apprunner_auto_scaling_configuration_version.web.arn

  depends_on = [aws_iam_role_policy_attachment.apprunner_ecr]
}

# --- Optional custom domain + DNS -------------------------------------------

resource "aws_apprunner_custom_domain_association" "domain" {
  count = var.custom_domain != "" ? 1 : 0

  domain_name = var.custom_domain
  service_arn = aws_apprunner_service.web.arn
}

# Point the apex/domain at the App Runner endpoint.
resource "aws_route53_record" "alias" {
  count = var.custom_domain != "" && var.manage_dns ? 1 : 0

  zone_id = var.route53_zone_id
  name    = var.custom_domain
  type    = "CNAME"
  ttl     = 300
  records = [aws_apprunner_custom_domain_association.domain[0].dns_target]
}

# ACM domain-validation records App Runner asks for.
resource "aws_route53_record" "validation" {
  for_each = (var.custom_domain != "" && var.manage_dns) ? {
    for r in aws_apprunner_custom_domain_association.domain[0].certificate_validation_records :
    r.name => r
  } : {}

  zone_id         = var.route53_zone_id
  name            = each.value.name
  type            = each.value.type
  ttl             = 300
  records         = [each.value.value]
  allow_overwrite = true
}
