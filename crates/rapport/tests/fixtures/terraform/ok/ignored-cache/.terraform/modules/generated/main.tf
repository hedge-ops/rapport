resource "null_resource" "generated" {
  triggers = {
    value = invalid_reference.missing.id
  }
}
