# Caranguejo leitor em Rust

Este projeto gera uma ilustração vetorial SVG de um caranguejo lendo um livro, com base na imagem de referência.

## Arquivos gerados

Ao executar o programa, ele gera:

- `caranguejo_leitor.svg` — versão com fundo bege e sombra, próxima da referência.
- `caranguejo_leitor_transparente.svg` — versão sem fundo.

## Como executar

```bash
cargo run --release
```

## Observações

- O código usa apenas a biblioteca padrão do Rust.
- O desenho é composto programaticamente por `path`, `circle`, `ellipse` e `rect` em SVG.
- A meta foi reproduzir o visual de forma fiel, mas ainda editável.
