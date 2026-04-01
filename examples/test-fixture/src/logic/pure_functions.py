# Laboratory layer — purity is enforced at the layer level.
# This file is in src/logic/ which belongs to the "laboratory" layer
# with rules: { purity: strict }. ALL functions here are checked.


# VIOLATION: calls print (stdout is forbidden in strict layer)
def calculate_total(items):
    total = sum(items)
    print(f"Total: {total}")  # forbidden
    return total


# VIOLATION: calls open (file_io is forbidden in strict layer)
def compute_from_file(path):
    data = open(path).read()  # forbidden
    return len(data)


# VIOLATION: save_results is in a strict layer — ALL functions checked
def save_results(results):
    print(results)
    with open("output.txt", "w") as f:
        f.write(str(results))


# OK: no IO operations
def add(a, b):
    return a + b


# VIOLATION: imports forbidden module (network_io)
def fetch_data(url):
    import requests  # forbidden
    return requests.get(url)
