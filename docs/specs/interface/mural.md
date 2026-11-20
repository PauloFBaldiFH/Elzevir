# Mural de recados

Recados da biblioteca guardados no servidor, mostrados no início da equipe e, quando públicos, no início do OPAC. Segue o mural do rascunho home-v11 (interface, §22 item 11, e §23 item 7).

## 1. Comportamento

1. **Recado.** Cada recado tem:
   - um texto de 1 a 1.400 caracteres, com espaços das pontas removidos e quebras de linha guardadas;
   - um tipo, um de quatro: Equipe, Institucional, Pessoal e Urgente;
   - um público, Equipe ou OPAC;
   - quem escreveu e o dia da última mudança, no fuso da biblioteca.

   O texto é só texto, nunca HTML.
2. **Limite.** O mural guarda no máximo **50 recados**. O 51.º é recusado com aviso, para o mural não virar arquivo.
3. **Quem mexe.**
   - Qualquer pessoa da equipe cria, edita e remove recados da equipe.
   - Recado do OPAC é publicado para o público, então criar, editar ou remover recado do OPAC, ou mudar um recado de público, pede permissão de administração.
   - O comando leva o público, e o servidor confere que ele bate com o recado guardado. Assim a permissão é decidida antes do comando chegar à thread escritora.
4. **Concorrência.** A edição leva a versão do recado (`row_version`). Se outra pessoa mudou o recado antes, a edição é recusada com aviso, e o texto digitado volta no formulário.
5. **Início da equipe.**
   - Mostra todos os recados, do mais novo para o mais antigo, com a cor do rascunho em rodízio: amarelo, azul, rosa e verde.
   - Cada recado mostra o tipo, o texto, o autor e o dia. Um recado do OPAC traz a marca "OPAC".
   - O botão "+ Nova nota" abre o formulário.
   - Cada recado tem o link "Editar"; a remoção fica no formulário de edição, para não acontecer por um clique solto.
   - Sem recados, aparece a nota de boas-vindas fixa.
6. **Início do OPAC.**
   - Mostra só os recados do OPAC, no máximo 6, do mais novo para o mais antigo.
   - Cada recado mostra o tipo, o texto e o dia, sem o nome de quem escreveu.
   - Sem recados públicos, aparece a nota de boas-vindas fixa.
7. **Rotas.**
   - `GET /equipe/mural/nova` abre o formulário.
   - `POST /equipe/mural` cria o recado.
   - `GET /equipe/mural/{id}` e `POST /equipe/mural/{id}` editam o recado.
   - `POST /equipe/mural/{id}/remover` remove o recado.
   - Todo `POST` leva o token CSRF da sessão. Recado inexistente responde 404, e falta de permissão responde 403.
8. **Auditoria.** Os comandos `board.add`, `board.update` e `board.remove` vão para o `audit_log` com quem os fez, como os demais.

## 2. Origem no Koha

Corresponde às notícias do Koha (`Koha::AdditionalContents`, ferramenta "News"), que têm local de exibição (equipe ou OPAC), título, conteúdo em HTML e validade. No Koha, a ferramenta toda pede `tools.edit_additional_contents`. Aqui o recado da equipe fica aberto a toda a equipe, como no rascunho, e só o recado público pede administração. O conteúdo é texto puro, para nenhum HTML de fora entrar no OPAC. Não é área de paridade.

## 3. Fora desta versão

- Validade, com data de publicação e de expiração.
- Recado por unidade e por idioma.
- Recado fixado no topo.
