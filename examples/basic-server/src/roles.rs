saps::construct_checks!(
    enum Role {
        Admin,
        Customer,
    }

    AdminRoleCheck => Admin,
    CustomerRoleCheck => Admin | Customer,
    NoRoleCheck => Admin | Customer,
);
