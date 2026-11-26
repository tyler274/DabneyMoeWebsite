# Optional GCP Billing Budget alert for dabney-moe-website.
#
# Enabled when var.billing_account_id is set (e.g. "01A29E-334D38-0B8604").
# Sends email alerts to billing account admins/users at 50%, 90%, 100% actual
# spend and 100% forecasted monthly spend.

resource "google_project_service" "billing_budgets" {
  count = var.billing_account_id != "" ? 1 : 0

  project            = var.project_id
  service            = "billingbudgets.googleapis.com"
  disable_on_destroy = false
}

data "google_project" "this" {
  count = var.billing_account_id != "" ? 1 : 0

  project_id = var.project_id
}

resource "google_billing_budget" "monthly" {
  count = var.billing_account_id != "" ? 1 : 0

  billing_account = var.billing_account_id
  display_name    = "${var.project_id} monthly budget (USD ${var.monthly_budget_usd})"

  budget_filter {
    projects = ["projects/${data.google_project.this[0].number}"]
  }

  amount {
    specified_amount {
      currency_code = "USD"
      units         = tostring(var.monthly_budget_usd)
    }
  }

  threshold_rules {
    threshold_percent = 0.5
    spend_basis       = "CURRENT_SPEND"
  }

  threshold_rules {
    threshold_percent = 0.9
    spend_basis       = "CURRENT_SPEND"
  }

  threshold_rules {
    threshold_percent = 1.0
    spend_basis       = "CURRENT_SPEND"
  }

  threshold_rules {
    threshold_percent = 1.0
    spend_basis       = "FORECASTED_SPEND"
  }

  depends_on = [google_project_service.billing_budgets]
}
