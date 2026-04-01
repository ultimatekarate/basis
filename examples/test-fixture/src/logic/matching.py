# Laboratory layer — exhaustive matching examples


# VIOLATION: missing Shipped, Delivered, Cancelled
def process_order_match(order_status):
    match order_status:
        case "Pending":
            return "waiting"
        case "Confirmed":
            return "confirmed"


# OK: all variants handled
def process_order_complete(order_status):
    match order_status:
        case "Pending":
            return "waiting"
        case "Confirmed":
            return "confirmed"
        case "Shipped":
            return "shipped"
        case "Delivered":
            return "delivered"
        case "Cancelled":
            return "cancelled"


# OK: wildcard covers remaining
def process_order_wildcard(order_status):
    match order_status:
        case "Pending":
            return "waiting"
        case _:
            return "other"


# VIOLATION: missing PayPal, BankTransfer
def handle_payment(payment_method):
    if payment_method == "CreditCard":
        charge_card()
    elif payment_method == "DebitCard":
        charge_debit()


# OK: has else clause
def handle_payment_with_else(payment_method):
    if payment_method == "CreditCard":
        charge_card()
    elif payment_method == "DebitCard":
        charge_debit()
    else:
        raise ValueError("unknown")


def charge_card():
    pass

def charge_debit():
    pass
