#ifndef CHECKSUM_H_
#define CHECKSUM_H_
/**
 * @file checksum.h
 * @brief 16-bit 1's complement checksum header, wrap carries aroudn
 * CREDIT: https://locklessinc.com/articles/tcp_checksum/
 */
unsigned short checksum1(const char *buf, unsigned int size);
unsigned short checksum3(const char *buf, unsigned int size);
int verify_checksum1(const char *buf, unsigned int size);
int verify_checksum3(const char *buf, unsigned int size);

#endif // !CHECKSUM_H_
