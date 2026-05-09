use std::process::Command;

// Интеграционные тесты
// Вызов происходить через cargo test
// TODO: расписать интеграционные тесты, чтобы тестировать случаи вызова программы

#[test]
fn test_my_cli_tool() {
    // Предположим, у вас есть собственный CLI инструмент
    let output = Command::new("cargo")
        .args(&["run", "--", "--help"])
        .output()
        .expect("Failed to run");

    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout).unwrap().contains("Usage"));
}
