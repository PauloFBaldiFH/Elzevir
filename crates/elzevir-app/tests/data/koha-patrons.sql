/*M!999999\- enable the sandbox mode */ 
-- MariaDB dump 10.19  Distrib 10.11.13-MariaDB, for debian-linux-gnu (x86_64)
--
-- Host: localhost    Database: koha_synthetic
-- ------------------------------------------------------
-- Patron 1's password is `Senha fictícia 1`; patron 6's, an old MD5 one, is not migrated.
-- A synthetic dump in the shape `koha-dump` writes (MariaDB 11), with fictitious patrons.

/*!40101 SET @OLD_CHARACTER_SET_CLIENT=@@CHARACTER_SET_CLIENT */;
/*!40101 SET NAMES utf8mb4 */;
/*!40103 SET TIME_ZONE='+00:00' */;
/*!40014 SET @OLD_UNIQUE_CHECKS=@@UNIQUE_CHECKS, UNIQUE_CHECKS=0 */;

--
-- Current Database: `koha_synthetic`
--

CREATE DATABASE /*!32312 IF NOT EXISTS*/ `koha_synthetic` /*!40100 DEFAULT CHARACTER SET utf8mb4 COLLATE utf8mb4_unicode_ci */;

USE `koha_synthetic`;

DROP TABLE IF EXISTS `branches`;
/*!40101 SET @saved_cs_client     = @@character_set_client */;
/*!40101 SET character_set_client = utf8mb4 */;
CREATE TABLE `branches` (
  `branchcode` varchar(10) NOT NULL DEFAULT '' COMMENT 'a unique key assigned to each branch',
  `branchname` longtext NOT NULL COMMENT 'the name of your library or branch',
  `branchaddress1` longtext DEFAULT NULL,
  `branchaddress2` longtext DEFAULT NULL,
  `branchaddress3` longtext DEFAULT NULL,
  `branchzip` varchar(25) DEFAULT NULL,
  `branchcity` longtext DEFAULT NULL,
  `branchstate` longtext DEFAULT NULL,
  `branchcountry` mediumtext DEFAULT NULL,
  `branchphone` longtext DEFAULT NULL,
  `branchfax` longtext DEFAULT NULL,
  `branchemail` longtext DEFAULT NULL,
  `branchurl` longtext DEFAULT NULL COMMENT 'the URL for your library or branch''s website',
  `pickup_location` tinyint(1) NOT NULL DEFAULT 1,
  PRIMARY KEY (`branchcode`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
/*!40101 SET character_set_client = @saved_cs_client */;

LOCK TABLES `branches` WRITE;
/*!40000 ALTER TABLE `branches` DISABLE KEYS */;
INSERT INTO `branches` VALUES ('CTR','Biblioteca Central','Rua das Flores, 100',NULL,'','85950-000','Cidade Exemplo','PR','Brasil','(44) 3000-0000',NULL,'central@example.org','https://biblioteca.example.org',1),('NRT','Ramal Norte',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,1);
/*!40000 ALTER TABLE `branches` ENABLE KEYS */;
UNLOCK TABLES;

DROP TABLE IF EXISTS `categories`;
CREATE TABLE `categories` (
  `categorycode` varchar(10) NOT NULL DEFAULT '',
  `description` longtext DEFAULT NULL,
  `enrolmentperiod` smallint(6) DEFAULT NULL,
  `enrolmentperioddate` date DEFAULT NULL,
  `enrolmentfee` decimal(28,6) DEFAULT NULL,
  `category_type` varchar(1) NOT NULL DEFAULT 'A',
  `default_privacy` enum('default','never','forever') NOT NULL DEFAULT 'default',
  PRIMARY KEY (`categorycode`)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

LOCK TABLES `categories` WRITE;
INSERT INTO `categories` VALUES ('ADU','Adulto',12,NULL,0.000000,'A','default'),('EST','Estudante',NULL,'2027-12-31',NULL,'C','never'),('INS',NULL,NULL,NULL,NULL,'I','default');
UNLOCK TABLES;

DROP TABLE IF EXISTS `cover_images`;
CREATE TABLE `cover_images` (
  `imagenumber` int(11) NOT NULL AUTO_INCREMENT,
  `mimetype` varchar(15) NOT NULL,
  `imagefile` mediumblob NOT NULL,
  `ratio` float DEFAULT NULL,
  `timestamp` timestamp NOT NULL DEFAULT current_timestamp() ON UPDATE current_timestamp(),
  PRIMARY KEY (`imagenumber`),
  KEY `bibnum` (`imagenumber`),
  CONSTRAINT `check_ratio` CHECK (`ratio` > 0)
) ENGINE=InnoDB AUTO_INCREMENT=3 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci COMMENT='images, with a quote''s comment';

LOCK TABLES `cover_images` WRITE;
INSERT INTO `cover_images` VALUES (1,'image/png',_binary '\0\Z\n\r\\\'',1.5e-05,'2024-01-01 00:00:00'),(2,'image/gif',0x47494638,-2.25E+3,'2024-01-02 00:00:00'),(3,'image/gif',X'4749',0,'2024-01-03 00:00:00');
UNLOCK TABLES;

DROP TABLE IF EXISTS `borrowers`;
CREATE TABLE `borrowers` (
  `borrowernumber` int(11) NOT NULL AUTO_INCREMENT COMMENT 'primary key',
  `cardnumber` varchar(32) DEFAULT NULL,
  `surname` longtext DEFAULT NULL,
  `firstname` mediumtext DEFAULT NULL,
  `title` longtext DEFAULT NULL,
  `streetnumber` tinytext DEFAULT NULL,
  `streettype` tinytext DEFAULT NULL,
  `address` longtext DEFAULT NULL,
  `address2` mediumtext DEFAULT NULL,
  `city` longtext DEFAULT NULL,
  `state` mediumtext DEFAULT NULL,
  `zipcode` tinytext DEFAULT NULL,
  `country` mediumtext DEFAULT NULL,
  `email` longtext DEFAULT NULL,
  `phone` mediumtext DEFAULT NULL,
  `mobile` tinytext DEFAULT NULL,
  `emailpro` mediumtext DEFAULT NULL,
  `phonepro` mediumtext DEFAULT NULL,
  `B_city` longtext DEFAULT NULL,
  `dateofbirth` date DEFAULT NULL,
  `branchcode` varchar(10) NOT NULL DEFAULT '',
  `categorycode` varchar(10) NOT NULL DEFAULT '',
  `dateenrolled` date DEFAULT NULL,
  `dateexpiry` date DEFAULT NULL,
  `gonenoaddress` tinyint(1) DEFAULT NULL,
  `borrowernotes` longtext DEFAULT NULL,
  `sex` varchar(1) DEFAULT NULL,
  `password` varchar(60) DEFAULT NULL,
  `flags` bigint(11) DEFAULT NULL,
  `userid` varchar(75) DEFAULT NULL,
  `privacy` int(11) NOT NULL DEFAULT 1,
  `updated_on` timestamp NOT NULL DEFAULT current_timestamp() ON UPDATE current_timestamp(),
  PRIMARY KEY (`borrowernumber`),
  UNIQUE KEY `cardnumber` (`cardnumber`),
  KEY `categorycode` (`categorycode`),
  CONSTRAINT `borrowers_ibfk_1` FOREIGN KEY (`categorycode`) REFERENCES `categories` (`categorycode`)
) ENGINE=InnoDB AUTO_INCREMENT=7 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

SET @OLD_AUTOCOMMIT=@@AUTOCOMMIT, @@AUTOCOMMIT=0;
LOCK TABLES `borrowers` WRITE;
/*!40000 ALTER TABLE `borrowers` DISABLE KEYS */;
INSERT INTO `borrowers` VALUES (1,'A0001','Silva','Ana Maria','Sra.','100','Rua','das Acácias','apto 2','Cidade Exemplo','PR','85950-000','Brasil','ana@example.org','(44) 99999-0001',NULL,NULL,NULL,'Outra Cidade','1990-05-10','CTR','ADU','2020-01-15','2030-01-15',0,'Leitora fictícia','F','$2a$08$KBCwKxOzLha2MUDgW0PjXeL8PLapJrHH7Qyn8x/oWtvIYu16Gx/s2',NULL,'ana',0,'2024-01-01 00:00:00'),(2,'A0002',NULL,'Escola Municipal Exemplo',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'sem-arroba',NULL,'(44) 3000-0002',NULL,NULL,NULL,'0000-00-00','NRT','INS',NULL,NULL,1,NULL,NULL,NULL,1,'escola',1,'2024-01-01 00:00:00'),(3,NULL,'Sem Carteirinha','Bruno',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'CTR','ADU','2021-03-01','2031-03-01',NULL,NULL,NULL,NULL,NULL,NULL,1,'2024-01-01 00:00:00'),(4,'A0004','Souza','Carla',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'XXX','ADU','2021-03-01','2031-03-01',NULL,NULL,NULL,NULL,NULL,NULL,1,'2024-01-01 00:00:00'),(5,'A0005','Lima','Davi',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'CTR','ZZ','2021-03-01','2031-03-01',NULL,NULL,NULL,NULL,NULL,NULL,2,'2024-01-01 00:00:00'),(6,'A0006','Pereira','Elisa',NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,NULL,'elisa@example.org',NULL,NULL,'2025-06-01','CTR','EST','2025-01-10','2024-12-31',NULL,NULL,NULL,'X03MO1qnZdYdgyfeuILPmQ',NULL,NULL,2,'2024-01-01 00:00:00');
/*!40000 ALTER TABLE `borrowers` ENABLE KEYS */;
UNLOCK TABLES;
COMMIT;
SET AUTOCOMMIT=@OLD_AUTOCOMMIT;

DROP TABLE IF EXISTS `borrower_debarments`;
CREATE TABLE `borrower_debarments` (
  `borrower_debarment_id` int(11) NOT NULL AUTO_INCREMENT,
  `borrowernumber` int(11) NOT NULL,
  `expiration` date DEFAULT NULL,
  `type` varchar(50) NOT NULL,
  `comment` mediumtext DEFAULT NULL,
  `manager_id` int(11) DEFAULT NULL,
  `created` timestamp NOT NULL DEFAULT current_timestamp() ON UPDATE current_timestamp(),
  `updated` timestamp NULL DEFAULT NULL,
  PRIMARY KEY (`borrower_debarment_id`)
) ENGINE=InnoDB AUTO_INCREMENT=6 DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;

LOCK TABLES `borrower_debarments` WRITE;
INSERT INTO `borrower_debarments` VALUES (1,1,'2099-01-01','SUSPENSION','Atraso',NULL,'2024-03-01 10:00:00',NULL),(2,1,'9999-12-31','MANUAL','Bloqueio',NULL,'2024-02-01 09:30:00',NULL),(3,1,NULL,'OVERDUES','Aviso',NULL,'2024-04-01 08:00:00',NULL),(4,6,'2000-01-01','SUSPENSION',NULL,NULL,'1999-12-01 08:00:00',NULL),(5,3,NULL,'MANUAL',NULL,NULL,'2024-01-01 08:00:00',NULL);
UNLOCK TABLES;
/*!40101 SET SQL_MODE=@OLD_SQL_MODE */;

-- Dump completed on 2026-11-26 12:00:00
