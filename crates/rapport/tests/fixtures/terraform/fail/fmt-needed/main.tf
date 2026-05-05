terraform {
  required_version = ">= 1.6.0"
}

resource "null_resource" "app" {
  triggers = {
    bad_format=true
  }
}
