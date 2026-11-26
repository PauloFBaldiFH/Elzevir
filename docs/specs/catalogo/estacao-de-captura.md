# Estação de captura: rascunhos das fontes na família nova (v0)

> Status: v0, tarefa 4.2. Base: [spec de interação](../../UI_BIBFRAME_INTERACTION_SPEC.md) §6.3 e §17.1, [fontes de rascunho](fontes-de-rascunho.md), [criação](criacao.md), [bipe](bipe.md) e [casamento](casamento.md). Critério da tarefa: **um rascunho aprovado vira família nova só depois de "Criar", com o MARC recebido como registro de origem e a origem `sru:<servidor>` na Instância; ISBN já cadastrado leva à grade existente**.

Quem cataloga um livro que a biblioteca não tem abre "+ Nova família", busca o livro nas fontes configuradas sem sair do formulário, escolhe um rascunho e confere os campos já preenchidos antes de criar.

## 1. Comportamento

### Estação

1. Com pelo menos uma fonte de rascunho habilitada ([fontes de rascunho](fontes-de-rascunho.md), item 18), o formulário da família nova tem o botão **"Buscar nas fontes"**. Sem fonte, o botão não aparece e as rotas desta spec respondem 404.
2. O botão troca o corpo do formulário pela **estação de captura**: os blocos Obra e Instância somem e voltam ao fechar ("← Voltar" ou `Esc`). Nada do que foi digitado no formulário se perde.
3. A estação tem os campos ISBN, título, autor e "qualquer campo", começando com o ISBN e o título já digitados no formulário. `Enter` num campo ou o botão **"Buscar"** consulta.
4. A estação lista as fontes na ordem da configuração. Cada fonte é pedida ao servidor **num pedido próprio**, todos ao mesmo tempo, e a resposta de cada uma aparece quando chega: uma fonte lenta não segura as outras (§17.1). Enquanto espera, a fonte mostra "buscando".
5. A resposta de uma fonte mostra:
   - **achados**: o total, uma linha por rascunho (título; autor, edição e publicação; ISBNs) com o botão **"✓ Aprovar"**, a contagem de registros ilegíveis e, quando há mais páginas, **"Próxima página"** (no máximo a página 50);
   - **nada achado**, quando o total é zero;
   - **sem resposta** ou **recusa** com a mensagem do servidor.
6. A família nova aberta pelo bipe de um ISBN que a base não tem ([bipe](bipe.md), item 4.3) já abre com a estação e a busca pelo ISBN feita (§6.3, item 2).

### Rotas

7. `GET /equipe/acervo/fontes/{n}?isbn=&title=&author=&any=&page=` consulta só a fonte `n` (de 0, na ordem da configuração) e devolve o trecho HTML da resposta. Consulta vazia ou longa demais responde 422 com a mensagem ([fontes de rascunho](fontes-de-rascunho.md), item 6); fonte inexistente, 404.
8. `GET /equipe/acervo/nova?rascunho={chave}` devolve o formulário da família nova preenchido com o rascunho (item 11).
9. `POST /equipe/acervo/nova` aceita no corpo `"draft": "{chave}"`, que liga a família criada ao rascunho (item 13).
10. As três rotas pedem sessão e a permissão `cataloguing`, como a criação ([criação](criacao.md), item 7).

### Aprovar

11. "✓ Aprovar" **não grava nada**: abre o formulário da família nova preenchido pelo rascunho. O MARC passa pelo mesmo mapeamento da importação (`marc_to_bibframe`), menos:
    - os campos locais `9XX` (exemplares `952`, tipo `942`, número do registro `999` da outra biblioteca), que descrevem o acervo de quem emprestou o registro e não o livro;
    - os exemplares.
12. O formulário mostra a origem do rascunho ("Rascunho de <servidor>"). Se um ISBN do rascunho já está na base (chave de casamento, [casamento](casamento.md), item 3), o formulário avisa e leva à grade da Instância existente (§6.3, item 5), pelo mesmo caminho do bipe (`/equipe/acervo?q=<ISBN>`). Criar mesmo assim continua possível.
13. **Criar** grava a família como qualquer família nova ([criação](criacao.md)), com os campos como estão no formulário, e acrescenta o registro de origem: o MARC recebido em ISO 2709 (UTF-8), sem os `9XX`. A Instância leva a origem `sru:<servidor>` (`NewInstance.origin`, gravada em `instance_origins`, a mesma marca de procedência da importação). As chaves de casamento são as da família gravada.
14. Os rascunhos de uma busca ficam guardados **só na memória do servidor**, por chave aleatória, por até 2 horas e no máximo 1.000 de cada vez (o mais antigo sai primeiro). Chave desconhecida ou vencida: o formulário (item 8) responde 404 e a criação (item 9) responde 422 com "O rascunho expirou; busque de novo". O navegador nunca recebe nem envia o MARC.

### Registro (log)

15. Cada consulta a uma fonte registra no log operacional a **posição** da fonte na configuração, o resultado (`found`, `no_response`, `refused`) e o tempo em milissegundos. Nem os termos da consulta nem o nome do servidor vão ao log, que só aceita números (fontes de rascunho, item 22).

## 2. Origem no Koha

- Busca: `cataloguing/z3950_search.pl` (com `C4::Breeding::Z3950Search`), que abre numa janela à parte.
- Aprovar: `cataloguing/addbiblio.pl?breedingid=`, que abre o editor com o registro do reservatório (`import_records`); nada é gravado antes de "Salvar".
- Diferenças intencionais: a estação fica dentro do formulário, sem janela; o rascunho vive só na memória, sem reservatório; os `9XX` da fonte são descartados (o Koha os traz para o editor, e o `999` é sobrescrito ao salvar).

## 3. Paridade

`livre` (interface). Testes: `crates/elzevir-app/src/drafts.rs` (rascunho aprovado sem `9XX` nem exemplares) e `crates/elzevir-web/tests/staff_catalog.rs` (estação, resposta por fonte, formulário preenchido, aviso de ISBN repetido, criação com registro de origem e origem da Instância, chave vencida), com uma fonte de mentira e nunca a rede.

## 4. Fora desta versão

- Estação nos blocos de Obra e Instância já gravados (substituir ou completar a descrição) e na Autoridade (tarefa 4.4).
- Propor pendurar a Instância numa Obra existente (outra edição): falta uma regra de casamento de Obra.
- Ligar e desligar servidores pela tela ("⚙️ Servidores").
