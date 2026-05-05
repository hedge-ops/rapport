terraform {
  required_version = ">= 1.6.0"
}

resource "null_resource" "app" {
  triggers = {
    value = invalid_reference.missing.id
  }
}
