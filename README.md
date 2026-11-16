# Elzevir
A modern and lightweight library system

## Primeira inicialização

Com o binário disponível no terminal, execute em uma pasta da instalação:

```sh
elzevir init --name "Minha biblioteca"
elzevir serve --config elzevir.toml
```

`init` pede e confirma a senha sem mostrá-la, cria o administrador `admin`,
a pasta `data-local` e a configuração. Recusa destinos existentes e volumes
de rede. O servidor usa `http://127.0.0.1:8080` por padrão. Em desenvolvimento
com HTTP local, use `serve --config elzevir.toml --insecure-cookies`.

Para automação, use `init --password-stdin` com entrada redirecionada ou a
variável `ELZEVIR_STAFF_PASSWORD`. A [especificação da inicialização](docs/specs/operacao/inicializacao.md)
descreve as opções e os limites desta etapa. Instaladores e registro como
serviço serão entregues separadamente.
